use std::collections::HashMap;

use crate::error::{DbError, Result};
use crate::storage::RowId;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondaryIndex {
    pub name: String,
    pub column: String,
    pub column_index: usize,
    pub unique: bool,
    entries: HashMap<Value, Vec<RowId>>,
}

impl SecondaryIndex {
    pub fn new(
        name: impl Into<String>,
        column: impl Into<String>,
        column_index: usize,
        unique: bool,
    ) -> Self {
        Self {
            name: name.into(),
            column: column.into(),
            column_index,
            unique,
            entries: HashMap::new(),
        }
    }

    pub fn insert(&mut self, value: &Value, row_id: RowId) -> Result<()> {
        if value.is_null() {
            return Ok(());
        }

        let row_ids = self.entries.entry(value.clone()).or_default();

        if self.unique && !row_ids.is_empty() && !row_ids.contains(&row_id) {
            return Err(DbError::ConstraintViolation(format!(
                "unique index {} rejects duplicate key {}",
                self.name, value
            )));
        }

        if !row_ids.contains(&row_id) {
            row_ids.push(row_id);
        }

        Ok(())
    }

    pub fn remove(&mut self, value: &Value, row_id: RowId) {
        if let Some(row_ids) = self.entries.get_mut(value) {
            row_ids.retain(|existing| *existing != row_id);

            if row_ids.is_empty() {
                self.entries.remove(value);
            }
        }
    }

    pub fn update(&mut self, old_value: &Value, new_value: &Value, row_id: RowId) -> Result<()> {
        if old_value == new_value {
            return Ok(());
        }

        self.remove(old_value, row_id);
        self.insert(new_value, row_id)
    }

    pub fn probe(&self, value: &Value) -> Vec<RowId> {
        self.entries.get(value).cloned().unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
