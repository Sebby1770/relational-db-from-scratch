use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Value {
    Int(i64),
    Text(String),
    Bool(bool),
    Null,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "INT",
            Value::Text(_) => "TEXT",
            Value::Bool(_) => "BOOL",
            Value::Null => "NULL",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(value) => write!(f, "{value}"),
            Value::Text(value) => write!(f, "{value}"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Null => write!(f, "NULL"),
        }
    }
}
