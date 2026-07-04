use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::codec::{
    SNAPSHOT_MAGIC, decode_row_record, decode_schema_block, encode_index, encode_row_record,
    encode_schema,
};
use crate::db::Database;
use crate::error::{DbError, Result};
use crate::wal::WalRecord;

#[derive(Debug)]
pub struct Persistence {
    directory: PathBuf,
    snapshot_path: PathBuf,
    wal_path: PathBuf,
}

impl Persistence {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        fs::create_dir_all(&directory)?;

        Ok(Self {
            snapshot_path: directory.join("database.snapshot"),
            wal_path: directory.join("wal.log"),
            directory,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn load_or_empty(self) -> Result<Database> {
        let mut database = if self.snapshot_path.exists() {
            self.load_snapshot()?
        } else {
            Database::new()
        };
        self.replay_wal(&mut database)?;
        database.attach_persistence(self);
        Ok(database)
    }

    pub fn load_snapshot(&self) -> Result<Database> {
        let content = fs::read_to_string(&self.snapshot_path)?;
        let mut lines = content.lines();
        let header = lines
            .next()
            .ok_or_else(|| DbError::Storage("snapshot is empty".into()))?;
        if header != SNAPSHOT_MAGIC {
            return Err(DbError::Storage("invalid snapshot header".into()));
        }

        let mut database = Database::new();
        let mut schema_lines = Vec::new();

        for line in lines {
            if line.starts_with("TABLE\t") {
                if !schema_lines.is_empty() {
                    Self::apply_schema_block(&mut database, &schema_lines)?;
                    schema_lines.clear();
                }
                schema_lines.push(line.to_string());
                continue;
            }

            if line.starts_with("COLUMN\t") || line.starts_with("INDEX\t") {
                schema_lines.push(line.to_string());
                continue;
            }

            if line.starts_with("ROW\t") {
                if !schema_lines.is_empty() {
                    Self::apply_schema_block(&mut database, &schema_lines)?;
                    schema_lines.clear();
                }
                Self::apply_row_line(&mut database, line)?;
            }
        }

        if !schema_lines.is_empty() {
            Self::apply_schema_block(&mut database, &schema_lines)?;
        }

        Ok(database)
    }

    pub fn checkpoint(&self, database: &Database) -> Result<()> {
        let mut snapshot = File::create(&self.snapshot_path)?;
        writeln!(snapshot, "{SNAPSHOT_MAGIC}")?;

        let mut table_names = database.table_names();
        table_names.sort();

        for table_name in table_names {
            let table = database
                .tables
                .get(&table_name)
                .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;

            writeln!(snapshot, "{}", encode_schema(&table.schema))?;
            for definition in table.index_definitions() {
                writeln!(snapshot, "{}", encode_index(&table_name, &definition))?;
            }
            for (row_id, row) in table.persisted_rows() {
                writeln!(snapshot, "{}", encode_row_record(&table_name, row_id, &row))?;
            }
        }

        self.append_record(&WalRecord::Checkpoint)?;
        self.truncate_wal()?;
        Ok(())
    }

    pub fn append_sql(&self, sql: &str) -> Result<()> {
        self.append_record(&WalRecord::Statement {
            sql: sql.trim().to_string(),
        })
    }

    pub fn replay_wal(&self, database: &mut Database) -> Result<()> {
        if !self.wal_path.exists() {
            return Ok(());
        }

        let file = File::open(&self.wal_path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            match WalRecord::decode(&line)? {
                WalRecord::Checkpoint => break,
                WalRecord::Statement { sql } => {
                    database.execute_internal(&sql)?;
                }
                other => {
                    return Err(DbError::Storage(format!(
                        "unsupported WAL record during replay: {other:?}"
                    )));
                }
            }
        }

        Ok(())
    }

    fn apply_schema_block(database: &mut Database, lines: &[String]) -> Result<()> {
        let block = lines.join("\n");
        let (schema, indexes) = decode_schema_block(&block)?;
        let table_name = schema.name.clone();
        database.create_table(schema)?;
        for index in indexes {
            database.create_index(index.name, table_name.clone(), index.column, index.unique)?;
        }
        Ok(())
    }

    fn apply_row_line(database: &mut Database, line: &str) -> Result<()> {
        let table_name = line
            .split('\t')
            .nth(1)
            .ok_or_else(|| DbError::Storage("row missing table name".into()))?;
        let table = database
            .tables
            .get(table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.into()))?;
        let column_count = table.schema.columns.len();
        let (table_name, row_id, row) = decode_row_record(line, column_count)?;
        let table = database
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        table.insert_with_row_id(row_id, row)
    }

    fn append_record(&self, record: &WalRecord) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.wal_path)?;
        writeln!(file, "{}", record.encode())?;
        Ok(())
    }

    fn truncate_wal(&self) -> Result<()> {
        File::create(&self.wal_path)?;
        Ok(())
    }
}
