use std::collections::HashMap;

use crate::error::{DbError, Result};
use crate::execution::{QueryResult, execute_statement};
use crate::parser::parse_sql;
use crate::row::Row;
use crate::schema::{TableSchema, normalize_identifier};
use crate::storage::Table;
use crate::transaction::{Transaction, TransactionId, UndoRecord};

#[derive(Debug, Default)]
pub struct Database {
    pub(crate) tables: HashMap<String, Table>,
    pub(crate) active_transaction: Option<Transaction>,
    next_transaction_id: u64,
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

    pub fn create_index(
        &mut self,
        name: impl Into<String>,
        table_name: impl Into<String>,
        column: impl Into<String>,
        unique: bool,
    ) -> Result<()> {
        let index_name = normalize_identifier(&name.into());
        let table_name = normalize_identifier(&table_name.into());
        let table = self
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        table.create_index(index_name, column, unique)
    }

    pub fn insert(&mut self, table_name: &str, row: Row) -> Result<()> {
        let table_name = normalize_identifier(table_name);
        let table = self
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;

        table.insert(row)?;
        Ok(())
    }

    pub fn begin(&mut self) -> Result<TransactionId> {
        if self.active_transaction.is_some() {
            return Err(DbError::Transaction(
                "nested transactions are not supported".into(),
            ));
        }

        let transaction_id = TransactionId(self.next_transaction_id.max(1));
        self.next_transaction_id = transaction_id.0 + 1;
        self.active_transaction = Some(Transaction::new(transaction_id));
        Ok(transaction_id)
    }

    pub fn commit(&mut self) -> Result<TransactionId> {
        let transaction = self
            .active_transaction
            .take()
            .ok_or_else(|| DbError::Transaction("no active transaction".into()))?;
        Ok(transaction.id)
    }

    pub fn rollback(&mut self) -> Result<TransactionId> {
        let transaction = self
            .active_transaction
            .take()
            .ok_or_else(|| DbError::Transaction("no active transaction".into()))?;

        for undo in transaction.undo_log.iter().rev() {
            self.apply_undo(undo.clone())?;
        }

        Ok(transaction.id)
    }

    pub(crate) fn record_undo(&mut self, undo: UndoRecord) {
        if let Some(transaction) = &mut self.active_transaction {
            transaction.undo_log.push(undo);
        }
    }

    fn apply_undo(&mut self, undo: UndoRecord) -> Result<()> {
        match undo {
            UndoRecord::CreateTable { table } => {
                self.tables.remove(&table);
                Ok(())
            }
            UndoRecord::CreateIndex { table, index } => {
                let table = self
                    .tables
                    .get_mut(&table)
                    .ok_or_else(|| DbError::TableNotFound(table.clone()))?;
                table.drop_index(&index)
            }
            UndoRecord::Insert { table, row_id } => {
                let table = self
                    .tables
                    .get_mut(&table)
                    .ok_or_else(|| DbError::TableNotFound(table.clone()))?;
                table.delete_row(row_id).map(|_| ())
            }
            UndoRecord::Delete { table, stored } => {
                let table = self
                    .tables
                    .get_mut(&table)
                    .ok_or_else(|| DbError::TableNotFound(table.clone()))?;
                table.insert_with_row_id(stored.row_id, stored.row)
            }
            UndoRecord::Update {
                table,
                row_id,
                old_row,
            } => {
                let table = self
                    .tables
                    .get_mut(&table)
                    .ok_or_else(|| DbError::TableNotFound(table.clone()))?;
                table.replace_row(row_id, old_row).map(|_| ())
            }
        }
    }
}
