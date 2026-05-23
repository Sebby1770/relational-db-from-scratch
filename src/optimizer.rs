use std::collections::{HashMap, HashSet};

use crate::row::Row;
use crate::schema::TableSchema;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableStats {
    pub row_count: usize,
    pub distinct_values: HashMap<String, usize>,
}

impl TableStats {
    pub fn from_rows(schema: &TableSchema, rows: impl Iterator<Item = Row>) -> Self {
        let mut distinct_sets = schema
            .columns
            .iter()
            .map(|column| (column.name.clone(), HashSet::<Value>::new()))
            .collect::<HashMap<_, _>>();
        let mut row_count = 0;

        for row in rows {
            row_count += 1;
            for (index, column) in schema.columns.iter().enumerate() {
                if let Some(values) = distinct_sets.get_mut(&column.name) {
                    values.insert(row[index].clone());
                }
            }
        }

        let distinct_values = distinct_sets
            .into_iter()
            .map(|(column, values)| (column, values.len()))
            .collect();

        Self {
            row_count,
            distinct_values,
        }
    }

    pub fn estimate_equality_rows(&self, column: &str) -> usize {
        let distinct = self
            .distinct_values
            .get(column)
            .copied()
            .unwrap_or(1)
            .max(1);
        (self.row_count / distinct).max(1).min(self.row_count)
    }
}
