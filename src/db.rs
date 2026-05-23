use std::collections::HashMap;

use crate::error::{DbError, Result};
use crate::execution::{QueryResult, execute_statement};
use crate::parser::parse_sql;
use crate::row::Row;
use crate::schema::{TableSchema, normalize_identifier};
use crate::storage::Table;

#[derive(Debug, Default)]
pub struct Database {
    pub(crate) tables: HashMap<String, Table>,
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn execute(&mut self, sql: &str) -> Result<QueryResult> {
        let statement = parse_sql(sql)?;
        execute_statement(self, statement)
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<()> {
        let table_name = normalize_identifier(&schema.name);

        if self.tables.contains_key(&table_name) {
            return Err(DbError::TableExists(table_name));
        }

        self.tables.insert(table_name, Table::new(schema));
        Ok(())
    }

    pub fn insert(&mut self, table_name: &str, row: Row) -> Result<()> {
        let table_name = normalize_identifier(table_name);
        let table = self
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;

        table.insert(row)
    }
}
