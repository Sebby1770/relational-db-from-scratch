use std::fs;

use relational_db_from_scratch::{Database, QueryResult, Value};

#[test]
fn database_round_trips_through_snapshot_and_wal() {
    let temp = std::env::temp_dir().join(format!(
        "relational-db-persist-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp).unwrap();

    {
        let mut db = Database::open(&temp).unwrap();
        db.execute("CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);")
            .unwrap();
        db.execute("INSERT INTO users VALUES (1, 'ada@example.com');")
            .unwrap();
        db.execute("INSERT INTO users VALUES (2, 'grace@example.com');")
            .unwrap();
        db.execute("CHECKPOINT;").unwrap();
        db.execute("UPDATE users SET email = 'hopper@example.com' WHERE id = 2;")
            .unwrap();
    }

    let mut db = Database::open(&temp).unwrap();
    let result = db
        .execute("SELECT id, email FROM users ORDER BY id;")
        .unwrap();

    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into(), "email".into()],
            rows: vec![
                vec![Value::Int(1), Value::Text("ada@example.com".into())],
                vec![Value::Int(2), Value::Text("hopper@example.com".into())],
            ],
        }
    );

    let _ = fs::remove_dir_all(temp);
}
