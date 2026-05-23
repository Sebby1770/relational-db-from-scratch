use std::fmt;

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    Parse(String),
    TableExists(String),
    TableNotFound(String),
    ColumnExists(String),
    ColumnNotFound(String),
    ArityMismatch {
        expected: usize,
        got: usize,
    },
    TypeMismatch {
        column: String,
        expected: String,
        got: String,
    },
    InvalidStatement(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Parse(message) => write!(f, "parse error: {message}"),
            DbError::TableExists(table) => write!(f, "table already exists: {table}"),
            DbError::TableNotFound(table) => write!(f, "table not found: {table}"),
            DbError::ColumnExists(column) => write!(f, "column already exists: {column}"),
            DbError::ColumnNotFound(column) => write!(f, "column not found: {column}"),
            DbError::ArityMismatch { expected, got } => {
                write!(f, "row has {got} values but table expects {expected}")
            }
            DbError::TypeMismatch {
                column,
                expected,
                got,
            } => write!(
                f,
                "type mismatch for column {column}: expected {expected}, got {got}"
            ),
            DbError::InvalidStatement(message) => write!(f, "invalid statement: {message}"),
        }
    }
}

impl std::error::Error for DbError {}
