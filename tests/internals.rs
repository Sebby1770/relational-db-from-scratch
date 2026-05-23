use relational_db_from_scratch::bplus_tree::BPlusTree;
use relational_db_from_scratch::concurrency::{LockManager, LockMode};
use relational_db_from_scratch::optimizer::TableStats;
use relational_db_from_scratch::pager::{PageId, SlottedPage};
use relational_db_from_scratch::schema::{Column, DataType, TableSchema};
use relational_db_from_scratch::transaction::TransactionId;
use relational_db_from_scratch::value::Value;
use relational_db_from_scratch::wal::WalRecord;

#[test]
fn lock_manager_allows_shared_locks_and_rejects_conflicting_writes() {
    let mut locks = LockManager::new();
    let tx1 = TransactionId(1);
    let tx2 = TransactionId(2);

    locks.acquire(tx1, "users", LockMode::Shared).unwrap();
    locks.acquire(tx2, "users", LockMode::Shared).unwrap();

    assert!(locks.acquire(tx1, "users", LockMode::Exclusive).is_err());
    locks.release_all(tx2);
    locks.acquire(tx1, "users", LockMode::Exclusive).unwrap();
}

#[test]
fn slotted_page_inserts_reads_deletes_and_reuses_slots() {
    let mut page = SlottedPage::new(PageId(7));
    let first = page.insert(b"hello").unwrap();
    let second = page.insert(b"world").unwrap();

    assert_eq!(page.get(first), Some(&b"hello"[..]));
    assert_eq!(page.get(second), Some(&b"world"[..]));
    page.delete(first).unwrap();
    let reused = page.insert(b"again").unwrap();

    assert_eq!(reused, first);
    assert_eq!(page.live_records(), 2);
}

#[test]
fn bplus_tree_supports_search_and_range_scan_after_splits() {
    let mut tree = BPlusTree::new(3);

    for key in [10, 20, 5, 6, 12, 30, 7, 17] {
        tree.insert(key, key * 10);
    }

    assert_eq!(tree.get(&12), Some(&120));
    assert_eq!(tree.get(&99), None);
    assert!(tree.height() > 1);
    assert_eq!(
        tree.range(6..=17),
        vec![(6, 60), (7, 70), (10, 100), (12, 120), (17, 170)]
    );
}

#[test]
fn wal_records_round_trip_through_text_encoding() {
    let record = WalRecord::PageWrite {
        tx: 42,
        page_id: 9,
        bytes: vec![0xde, 0xad, 0xbe, 0xef],
    };

    let encoded = record.encode();
    assert_eq!(WalRecord::decode(&encoded).unwrap(), record);
    assert_eq!(
        WalRecord::decode("COMMIT|42").unwrap(),
        WalRecord::Commit { tx: 42 }
    );
}

#[test]
fn table_stats_estimate_equality_selectivity() {
    let schema = TableSchema::new(
        "users",
        vec![
            Column::new("id", DataType::Int).primary_key(),
            Column::new("active", DataType::Bool),
        ],
    )
    .unwrap();
    let rows = vec![
        vec![Value::Int(1), Value::Bool(true)],
        vec![Value::Int(2), Value::Bool(false)],
        vec![Value::Int(3), Value::Bool(true)],
        vec![Value::Int(4), Value::Bool(false)],
    ];

    let stats = TableStats::from_rows(&schema, rows.into_iter());

    assert_eq!(stats.row_count, 4);
    assert_eq!(stats.distinct_values["active"], 2);
    assert_eq!(stats.estimate_equality_rows("active"), 2);
}
