use std::collections::HashMap;

use crate::error::{DbError, Result};
use crate::index::SecondaryIndex;
use crate::row::Row;
use crate::schema::{TableSchema, normalize_identifier};

pub type RowId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRow {
    pub row_id: RowId,
    pub row: Row,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub schema: TableSchema,
    next_row_id: RowId,
    rows: HashMap<RowId, Row>,
    indexes: HashMap<String, SecondaryIndex>,
}

impl Table {
    pub fn new(schema: TableSchema) -> Self {
        Self {
            schema,
            next_row_id: 1,
            rows: HashMap::new(),
            indexes: HashMap::new(),
        }
    }

    pub fn insert(&mut self, row: Row) -> Result<RowId> {
        self.validate_row_for_write(&row, None)?;
        let row_id = self.next_row_id;
        self.next_row_id += 1;
        self.insert_with_row_id(row_id, row)?;
        Ok(row_id)
    }

    pub fn insert_with_row_id(&mut self, row_id: RowId, row: Row) -> Result<()> {
        self.validate_row_for_write(&row, Some(row_id))?;

        for index in self.indexes.values_mut() {
            index.insert(&row[index.column_index], row_id)?;
        }

        if row_id >= self.next_row_id {
            self.next_row_id = row_id + 1;
        }

        self.rows.insert(row_id, row);
        Ok(())
    }

    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sorted_row_ids()
            .into_iter()
            .map(|row_id| self.rows.get(&row_id).expect("sorted row id exists"))
    }

    pub fn stored_rows(&self) -> impl Iterator<Item = StoredRow> + '_ {
        self.sorted_row_ids().into_iter().map(|row_id| StoredRow {
            row_id,
            row: self
                .rows
                .get(&row_id)
                .expect("sorted row id exists")
                .clone(),
        })
    }

    pub fn row(&self, row_id: RowId) -> Option<&Row> {
        self.rows.get(&row_id)
    }

    pub fn row_ids(&self) -> Vec<RowId> {
        self.sorted_row_ids()
    }

    pub fn replace_row(&mut self, row_id: RowId, new_row: Row) -> Result<Row> {
        self.validate_row_for_write(&new_row, Some(row_id))?;
        let old_row = self
            .rows
            .get(&row_id)
            .ok_or_else(|| DbError::InvalidStatement(format!("row id {row_id} not found")))?
            .clone();

        for index in self.indexes.values_mut() {
            index.update(
                &old_row[index.column_index],
                &new_row[index.column_index],
                row_id,
            )?;
        }

        self.rows.insert(row_id, new_row);
        Ok(old_row)
    }

    pub fn delete_row(&mut self, row_id: RowId) -> Result<StoredRow> {
        let row = self
            .rows
            .remove(&row_id)
            .ok_or_else(|| DbError::InvalidStatement(format!("row id {row_id} not found")))?;

        for index in self.indexes.values_mut() {
            index.remove(&row[index.column_index], row_id);
        }

        Ok(StoredRow { row_id, row })
    }

    pub fn create_index(
        &mut self,
        name: impl Into<String>,
        column: impl Into<String>,
        unique: bool,
    ) -> Result<()> {
        let name = normalize_identifier(&name.into());
        let column = normalize_identifier(&column.into());

        if self.indexes.contains_key(&name) {
            return Err(DbError::IndexExists(name));
        }

        let column_index = self
            .schema
            .column_index(&column)
            .ok_or_else(|| DbError::ColumnNotFound(column.clone()))?;
        let mut index = SecondaryIndex::new(name.clone(), column, column_index, unique);

        for (row_id, row) in &self.rows {
            index.insert(&row[column_index], *row_id)?;
        }

        self.indexes.insert(name, index);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        let name = normalize_identifier(name);
        self.indexes
            .remove(&name)
            .map(|_| ())
            .ok_or(DbError::IndexNotFound(name))
    }

    pub fn index_on_column(&self, column_index: usize) -> Option<&SecondaryIndex> {
        let mut matches = self
            .indexes
            .values()
            .filter(|index| index.column_index == column_index)
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.name.cmp(&right.name));
        matches.first().copied()
    }

    pub fn index(&self, name: &str) -> Option<&SecondaryIndex> {
        self.indexes.get(&normalize_identifier(name))
    }

    pub fn index_names(&self) -> Vec<String> {
        let mut names = self.indexes.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    fn sorted_row_ids(&self) -> Vec<RowId> {
        let mut ids = self.rows.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    fn validate_row_for_write(&self, row: &Row, replacing_row_id: Option<RowId>) -> Result<()> {
        self.schema.validate_row(row)?;

        for column_index in self.schema.unique_column_indexes() {
            let value = &row[column_index];

            if value.is_null() {
                continue;
            }

            let column_name = &self.schema.columns[column_index].name;
            let duplicate = self.rows.iter().any(|(row_id, stored)| {
                Some(*row_id) != replacing_row_id && stored[column_index] == *value
            });

            if duplicate {
                return Err(DbError::ConstraintViolation(format!(
                    "unique column {column_name} rejects duplicate key {value}"
                )));
            }
        }

        for index in self.indexes.values() {
            if !index.unique {
                continue;
            }

            let value = &row[index.column_index];
            if value.is_null() {
                continue;
            }

            let duplicate = self.rows.iter().any(|(row_id, stored)| {
                Some(*row_id) != replacing_row_id && stored[index.column_index] == *value
            });

            if duplicate {
                return Err(DbError::ConstraintViolation(format!(
                    "unique index {} rejects duplicate key {}",
                    index.name, value
                )));
            }
        }

        Ok(())
    }
}
