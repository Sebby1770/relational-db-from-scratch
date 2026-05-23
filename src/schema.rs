use std::collections::HashSet;
use std::fmt;

use crate::error::{DbError, Result};
use crate::row::Row;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataType {
    Int,
    Text,
    Bool,
}

impl DataType {
    pub fn accepts(&self, value: &Value) -> bool {
        matches!(
            (self, value),
            (DataType::Int, Value::Int(_))
                | (DataType::Text, Value::Text(_))
                | (DataType::Bool, Value::Bool(_))
        )
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Int => write!(f, "INT"),
            DataType::Text => write!(f, "TEXT"),
            DataType::Bool => write!(f, "BOOL"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
}

impl Column {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: normalize_identifier(&name.into()),
            data_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
}

impl TableSchema {
    pub fn new(name: impl Into<String>, columns: Vec<Column>) -> Result<Self> {
        let columns = columns
            .into_iter()
            .map(|mut column| {
                column.name = normalize_identifier(&column.name);
                column
            })
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();

        for column in &columns {
            if !seen.insert(column.name.clone()) {
                return Err(DbError::ColumnExists(column.name.clone()));
            }
        }

        Ok(Self {
            name: normalize_identifier(&name.into()),
            columns,
        })
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        let name = normalize_identifier(name);
        self.columns.iter().position(|column| column.name == name)
    }

    pub fn column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| column.name.clone())
            .collect()
    }

    pub fn validate_row(&self, row: &Row) -> Result<()> {
        if self.columns.len() != row.len() {
            return Err(DbError::ArityMismatch {
                expected: self.columns.len(),
                got: row.len(),
            });
        }

        for (index, value) in row.iter().enumerate() {
            self.validate_value(index, value)?;
        }

        Ok(())
    }

    pub fn validate_value(&self, column_index: usize, value: &Value) -> Result<()> {
        let column = &self.columns[column_index];

        if !column.data_type.accepts(value) {
            return Err(DbError::TypeMismatch {
                column: column.name.clone(),
                expected: column.data_type.to_string(),
                got: value.type_name().to_string(),
            });
        }

        Ok(())
    }
}

pub(crate) fn normalize_identifier(identifier: &str) -> String {
    identifier.trim().to_ascii_lowercase()
}
