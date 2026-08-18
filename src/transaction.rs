use crate::row::Row;
use crate::storage::{RowId, StoredRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransactionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoRecord {
    CreateTable {
        table: String,
    },
    CreateIndex {
        table: String,
        index: String,
    },
    Insert {
        table: String,
        row_id: RowId,
    },
    Delete {
        table: String,
        stored: StoredRow,
    },
    Update {
        table: String,
        row_id: RowId,
        old_row: Row,
    },
    AddColumn {
        table: String,
    },
    RenameTable {
        from: String,
        to: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub id: TransactionId,
    pub undo_log: Vec<UndoRecord>,
}

impl Transaction {
    pub fn new(id: TransactionId) -> Self {
        Self {
            id,
            undo_log: Vec::new(),
        }
    }
}
