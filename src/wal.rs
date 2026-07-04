use crate::error::{DbError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalRecord {
    Begin {
        tx: u64,
    },
    PageWrite {
        tx: u64,
        page_id: u64,
        bytes: Vec<u8>,
    },
    Commit {
        tx: u64,
    },
    Rollback {
        tx: u64,
    },
    Statement {
        sql: String,
    },
    Checkpoint,
}

impl WalRecord {
    pub fn encode(&self) -> String {
        match self {
            WalRecord::Begin { tx } => format!("BEGIN|{tx}"),
            WalRecord::PageWrite { tx, page_id, bytes } => {
                format!("PAGE|{tx}|{page_id}|{}", encode_hex(bytes))
            }
            WalRecord::Commit { tx } => format!("COMMIT|{tx}"),
            WalRecord::Rollback { tx } => format!("ROLLBACK|{tx}"),
            WalRecord::Statement { sql } => format!("SQL|{}", encode_sql(sql)),
            WalRecord::Checkpoint => "CHECKPOINT".into(),
        }
    }

    pub fn decode(input: &str) -> Result<Self> {
        let parts = input.split('|').collect::<Vec<_>>();

        match parts.as_slice() {
            ["BEGIN", tx] => Ok(WalRecord::Begin { tx: parse_u64(tx)? }),
            ["PAGE", tx, page_id, bytes] => Ok(WalRecord::PageWrite {
                tx: parse_u64(tx)?,
                page_id: parse_u64(page_id)?,
                bytes: decode_hex(bytes)?,
            }),
            ["COMMIT", tx] => Ok(WalRecord::Commit { tx: parse_u64(tx)? }),
            ["ROLLBACK", tx] => Ok(WalRecord::Rollback { tx: parse_u64(tx)? }),
            ["SQL", sql] => Ok(WalRecord::Statement {
                sql: decode_sql(sql)?,
            }),
            ["CHECKPOINT"] => Ok(WalRecord::Checkpoint),
            _ => Err(DbError::Storage(format!("invalid WAL record: {input}"))),
        }
    }
}

fn parse_u64(input: &str) -> Result<u64> {
    input
        .parse::<u64>()
        .map_err(|_| DbError::Storage(format!("invalid WAL integer: {input}")))
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn encode_sql(sql: &str) -> String {
    sql.replace('\\', "\\\\").replace('|', "\\p")
}

fn decode_sql(input: &str) -> Result<String> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => output.push('\\'),
                Some('p') => output.push('|'),
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
    Ok(output)
}

fn decode_hex(input: &str) -> Result<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        return Err(DbError::Storage("hex payload must have even length".into()));
    }

    input
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let raw = std::str::from_utf8(pair)
                .map_err(|_| DbError::Storage("invalid utf-8 in hex payload".into()))?;
            u8::from_str_radix(raw, 16)
                .map_err(|_| DbError::Storage(format!("invalid hex byte: {raw}")))
        })
        .collect()
}
