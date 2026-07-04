use crate::error::{DbError, Result};
use crate::row::Row;
use crate::schema::{Column, DataType, TableSchema};
use crate::storage::RowId;
use crate::value::Value;

pub const SNAPSHOT_MAGIC: &str = "RDBFS1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDefinition {
    pub name: String,
    pub column: String,
    pub unique: bool,
}

pub fn encode_value(value: &Value) -> String {
    match value {
        Value::Int(number) => format!("I:{number}"),
        Value::Text(text) => format!("T:{}", escape_text(text)),
        Value::Bool(flag) => format!("B:{flag}"),
        Value::Null => "N".into(),
    }
}

pub fn decode_value(input: &str) -> Result<Value> {
    if input == "N" {
        return Ok(Value::Null);
    }

    let (tag, payload) = input
        .split_once(':')
        .ok_or_else(|| DbError::Storage(format!("invalid encoded value: {input}")))?;

    match tag {
        "I" => payload
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| DbError::Storage(format!("invalid integer value: {input}"))),
        "T" => Ok(Value::Text(unescape_text(payload))),
        "B" => match payload {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            other => Err(DbError::Storage(format!("invalid bool value: {other}"))),
        },
        _ => Err(DbError::Storage(format!("unknown value tag: {tag}"))),
    }
}

pub fn encode_row(row: &Row) -> String {
    row.iter().map(encode_value).collect::<Vec<_>>().join("\t")
}

pub fn decode_row(input: &str, expected: usize) -> Result<Row> {
    if input.is_empty() && expected == 0 {
        return Ok(Vec::new());
    }

    let values = input
        .split('\t')
        .map(decode_value)
        .collect::<Result<Row>>()?;
    if values.len() != expected {
        return Err(DbError::Storage(format!(
            "row has {} values but expected {expected}",
            values.len()
        )));
    }

    Ok(values)
}

pub fn encode_schema(schema: &TableSchema) -> String {
    let mut lines = vec![format!("TABLE\t{}", schema.name)];
    for column in &schema.columns {
        let mut flags = Vec::new();
        if column.primary_key {
            flags.push("PK");
        }
        if column.unique {
            flags.push("UQ");
        }
        if !column.nullable {
            flags.push("NN");
        }
        lines.push(format!(
            "COLUMN\t{}\t{}\t{}",
            column.name,
            column.data_type,
            flags.join(",")
        ));
    }
    lines.join("\n")
}

pub fn decode_schema_block(block: &str) -> Result<(TableSchema, Vec<IndexDefinition>)> {
    let mut lines = block.lines();
    let header = lines
        .next()
        .ok_or_else(|| DbError::Storage("schema block is empty".into()))?;
    let table_name = header
        .strip_prefix("TABLE\t")
        .ok_or_else(|| DbError::Storage("invalid schema header".into()))?;

    let mut columns = Vec::new();
    let mut indexes = Vec::new();

    for line in lines {
        if let Some(rest) = line.strip_prefix("COLUMN\t") {
            let parts = rest.split('\t').collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err(DbError::Storage(format!("invalid column line: {line}")));
            }

            let mut column = Column::new(parts[0], parse_data_type(parts[1])?);
            for flag in parts[2].split(',') {
                match flag {
                    "PK" => column = column.primary_key(),
                    "UQ" => column = column.unique(),
                    "NN" => column = column.not_null(),
                    "" => {}
                    other => {
                        return Err(DbError::Storage(format!("unknown column flag: {other}")));
                    }
                }
            }
            columns.push(column);
            continue;
        }

        if let Some(rest) = line.strip_prefix("INDEX\t") {
            let parts = rest.split('\t').collect::<Vec<_>>();
            if parts.len() != 4 {
                return Err(DbError::Storage(format!("invalid index line: {line}")));
            }
            indexes.push(IndexDefinition {
                name: parts[0].into(),
                column: parts[2].into(),
                unique: parts[3] == "1",
            });
        }
    }

    Ok((TableSchema::new(table_name, columns)?, indexes))
}

pub fn encode_index(table: &str, definition: &IndexDefinition) -> String {
    format!(
        "INDEX\t{}\t{}\t{}\t{}",
        definition.name,
        table,
        definition.column,
        if definition.unique { "1" } else { "0" }
    )
}

pub fn encode_row_record(table: &str, row_id: RowId, row: &Row) -> String {
    format!("ROW\t{table}\t{row_id}\t{}", encode_row(row))
}

pub fn decode_row_record(line: &str, expected_columns: usize) -> Result<(String, RowId, Row)> {
    let parts = line.split('\t').collect::<Vec<_>>();
    if parts.len() < 4 || parts[0] != "ROW" {
        return Err(DbError::Storage(format!("invalid row record: {line}")));
    }

    let table = parts[1].to_string();
    let row_id = parts[2]
        .parse::<RowId>()
        .map_err(|_| DbError::Storage(format!("invalid row id: {}", parts[2])))?;
    let row = decode_row(&parts[3..].join("\t"), expected_columns)?;
    Ok((table, row_id, row))
}

fn parse_data_type(input: &str) -> Result<DataType> {
    match input {
        "INT" => Ok(DataType::Int),
        "TEXT" => Ok(DataType::Text),
        "BOOL" => Ok(DataType::Bool),
        other => Err(DbError::Storage(format!("unknown data type: {other}"))),
    }
}

fn escape_text(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn unescape_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => output.push('\\'),
                Some('n') => output.push('\n'),
                Some('t') => output.push('\t'),
                Some(other) => {
                    output.push('\\');
                    output.push(other);
                }
                None => output.push('\\'),
            }
        } else {
            output.push(ch);
        }
    }
    output
}
