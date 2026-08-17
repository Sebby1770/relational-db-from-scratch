use std::collections::HashMap;
use std::path::Path;

use crate::concurrency::{LockManager, LockMode};
use crate::error::{DbError, Result};
use crate::execution::{QueryResult, execute_statement};
use crate::optimizer::TableStats;
use crate::parser::parse_sql;
use crate::persistence::Persistence;
use crate::row::Row;
use crate::schema::{TableSchema, normalize_identifier};
use crate::storage::Table;
use crate::transaction::{Transaction, TransactionId, UndoRecord};

#[derive(Debug)]
pub struct Database {
    pub(crate) tables: HashMap<String, Table>,
    pub(crate) stats: HashMap<String, TableStats>,
    pub(crate) lock_manager: LockManager,
    pub(crate) active_transaction: Option<Transaction>,
    persistence: Option<Persistence>,
    next_transaction_id: u64,
}

impl Default for Database {
    fn default() -> Self {
        Self {
            tables: HashMap::new(),
            stats: HashMap::new(),
            lock_manager: LockManager::new(),
            active_transaction: None,
            persistence: None,
            next_transaction_id: 0,
        }
    }
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        Persistence::open(directory)?.load_or_empty()
    }

    pub fn data_directory(&self) -> Option<&Path> {
        self.persistence.as_ref().map(Persistence::directory)
    }

    pub fn execute(&mut self, sql: &str) -> Result<QueryResult> {
        let statement = parse_sql(sql)?;
        let result = execute_statement(self, statement)?;
        if Self::should_log_sql(sql)
            && let Some(persistence) = &self.persistence
        {
            persistence.append_sql(sql)?;
        }
        Ok(result)
    }

    pub fn import_csv(&mut self, path: impl AsRef<Path>, table: &str) -> Result<QueryResult> {
        let path = path.as_ref().to_string_lossy().replace('\'', "''");
        self.execute(&format!("COPY {table} FROM '{path}'"))
    }

    pub(crate) fn execute_internal(&mut self, sql: &str) -> Result<QueryResult> {
        let statement = parse_sql(sql)?;
        execute_statement(self, statement)
    }

    pub(crate) fn attach_persistence(&mut self, persistence: Persistence) {
        self.persistence = Some(persistence);
    }

    pub(crate) fn checkpoint(&self) -> Result<String> {
        if let Some(persistence) = &self.persistence {
            persistence.checkpoint(self)?;
            Ok(format!(
                "checkpoint complete at {}",
                persistence.directory().display()
            ))
        } else {
            Ok("checkpoint requested; open the database with a data directory first".into())
        }
    }

    pub fn table_names(&self) -> Vec<String> {
        let mut names = self.tables.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn describe_table(&self, table_name: &str) -> Result<String> {
        let table_name = normalize_identifier(table_name);
        let table = self
            .tables
            .get(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;

        let mut lines = vec![format!("table {table_name}")];
        for column in &table.schema.columns {
            let mut flags = Vec::new();
            if column.primary_key {
                flags.push("PRIMARY KEY");
            }
            if column.unique {
                flags.push("UNIQUE");
            }
            if !column.nullable {
                flags.push("NOT NULL");
            }
            let suffix = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(", "))
            };
            lines.push(format!("  {} {}{}", column.name, column.data_type, suffix));
        }

        let indexes = table.index_names();
        if !indexes.is_empty() {
            lines.push("indexes:".into());
            for index_name in indexes {
                if let Some(index) = table.index(index_name.as_str()) {
                    let unique = if index.unique { "unique " } else { "" };
                    lines.push(format!("  {}{} on {}", unique, index.name, index.column));
                }
            }
        }

        if let Some(stats) = self.stats.get(&table_name) {
            lines.push(format!("rows: {}", stats.row_count));
        }

        if let Some(directory) = self.data_directory() {
            lines.push(format!("storage: {}", directory.display()));
        }

        Ok(lines.join("\n"))
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

    pub fn drop_table(&mut self, table_name: &str) -> Result<()> {
        let table_name = normalize_identifier(table_name);
        self.tables
            .remove(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        self.stats.remove(&table_name);
        Ok(())
    }

    pub fn table_for_index(&self, index_name: &str) -> Result<String> {
        let index_name = normalize_identifier(index_name);
        for (table_name, table) in &self.tables {
            if table.index(&index_name).is_some() {
                return Ok(table_name.clone());
            }
        }

        Err(DbError::IndexNotFound(index_name))
    }

    pub fn drop_index(&mut self, index_name: &str) -> Result<(String, String)> {
        let index_name = normalize_identifier(index_name);
        let table_name = self.table_for_index(&index_name)?;
        self.tables
            .get_mut(&table_name)
            .expect("table exists")
            .drop_index(&index_name)?;
        Ok((table_name, index_name))
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
        self.lock_manager.release_all(transaction.id);
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

        self.lock_manager.release_all(transaction.id);
        Ok(transaction.id)
    }

    pub(crate) fn record_undo(&mut self, undo: UndoRecord) {
        if let Some(transaction) = &mut self.active_transaction {
            transaction.undo_log.push(undo);
        }
    }

    pub(crate) fn acquire_read_lock(&mut self, resource: &str) -> Result<()> {
        if let Some(transaction_id) = self.active_transaction_id() {
            self.lock_manager
                .acquire(transaction_id, resource, LockMode::Shared)?;
        }
        Ok(())
    }

    pub(crate) fn acquire_write_lock(&mut self, resource: &str) -> Result<()> {
        if let Some(transaction_id) = self.active_transaction_id() {
            self.lock_manager
                .acquire(transaction_id, resource, LockMode::Exclusive)?;
        }
        Ok(())
    }

    fn active_transaction_id(&self) -> Option<TransactionId> {
        self.active_transaction
            .as_ref()
            .map(|transaction| transaction.id)
    }

    fn should_log_sql(sql: &str) -> bool {
        let trimmed = sql.trim().to_ascii_uppercase();
        !(trimmed.starts_with("SELECT")
            || trimmed.starts_with("EXPLAIN")
            || trimmed.starts_with("ANALYZE")
            || trimmed.starts_with("CHECKPOINT"))
    }

    fn apply_undo(&mut self, undo: UndoRecord) -> Result<()> {
        match undo {
            UndoRecord::CreateTable { table } => {
                self.tables.remove(&table);
                self.stats.remove(&table);
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
