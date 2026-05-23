use crate::error::Result;
use crate::row::Row;
use crate::schema::TableSchema;

#[derive(Debug, Clone)]
pub struct Table {
    pub schema: TableSchema,
    rows: Vec<Row>,
}

impl Table {
    pub fn new(schema: TableSchema) -> Self {
        Self {
            schema,
            rows: Vec::new(),
        }
    }

    pub fn insert(&mut self, row: Row) -> Result<()> {
        self.schema.validate_row(&row)?;
        self.rows.push(row);
        Ok(())
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn rows_mut(&mut self) -> &mut [Row] {
        &mut self.rows
    }

    pub fn delete_where<F>(&mut self, mut should_delete: F) -> usize
    where
        F: FnMut(&Row) -> bool,
    {
        let before = self.rows.len();
        self.rows.retain(|row| !should_delete(row));
        before - self.rows.len()
    }
}
