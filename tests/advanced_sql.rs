use relational_db_from_scratch::{Database, QueryResult, Value};

#[test]
fn constraints_reject_duplicate_primary_keys_and_null_not_null_values() {
    let mut db = Database::new();
    db.execute(
        "CREATE TABLE users (
            id INT PRIMARY KEY,
            email TEXT UNIQUE NOT NULL,
            active BOOL
        );",
    )
    .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'ada@example.com', true);")
        .unwrap();

    assert!(
        db.execute("INSERT INTO users VALUES (1, 'other@example.com', true);")
            .is_err()
    );
    assert!(
        db.execute("INSERT INTO users VALUES (2, null, true);")
            .is_err()
    );
}

#[test]
fn indexes_are_used_by_explain_and_maintained_on_update_delete() {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE, active BOOL);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'ada@example.com', true);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (2, 'grace@example.com', false);")
        .unwrap();
    db.execute("CREATE INDEX users_email_idx ON users(email);")
        .unwrap();

    let plan = db
        .execute("EXPLAIN SELECT id FROM users WHERE email = 'ada@example.com';")
        .unwrap();
    assert!(matches!(plan, QueryResult::Plan { ref plan } if plan.contains("IndexLookup")));

    db.execute("UPDATE users SET email = 'hopper@example.com' WHERE id = 2;")
        .unwrap();
    db.execute("DELETE FROM users WHERE email = 'ada@example.com';")
        .unwrap();

    let result = db
        .execute("SELECT id FROM users WHERE email = 'hopper@example.com';")
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(2)]],
        }
    );
}

#[test]
fn predicates_sort_limit_and_count_work_together() {
    let mut db = seeded_scores();

    let result = db
        .execute(
            "SELECT id FROM scores WHERE points >= 20 AND active = true ORDER BY id DESC LIMIT 2;",
        )
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(4)], vec![Value::Int(3)]],
        }
    );

    let count = db
        .execute("SELECT COUNT(*) FROM scores WHERE points > 10;")
        .unwrap();
    assert_eq!(
        count,
        QueryResult::Rows {
            columns: vec!["count".into()],
            rows: vec![vec![Value::Int(3)]],
        }
    );
}

#[test]
fn rollback_restores_insert_update_and_delete_changes() {
    let mut db = seeded_scores();

    db.execute("BEGIN;").unwrap();
    db.execute("INSERT INTO scores VALUES (5, 50, true);")
        .unwrap();
    db.execute("UPDATE scores SET points = 99 WHERE id = 1;")
        .unwrap();
    db.execute("DELETE FROM scores WHERE id = 2;").unwrap();
    db.execute("ROLLBACK;").unwrap();

    let result = db
        .execute("SELECT id, points FROM scores ORDER BY id;")
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into(), "points".into()],
            rows: vec![
                vec![Value::Int(1), Value::Int(10)],
                vec![Value::Int(2), Value::Int(20)],
                vec![Value::Int(3), Value::Int(30)],
                vec![Value::Int(4), Value::Int(40)],
            ],
        }
    );
}

#[test]
fn commit_keeps_transaction_changes() {
    let mut db = seeded_scores();

    db.execute("BEGIN;").unwrap();
    db.execute("UPDATE scores SET active = true WHERE id = 2;")
        .unwrap();
    db.execute("COMMIT;").unwrap();

    let result = db
        .execute("SELECT active FROM scores WHERE id = 2;")
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["active".into()],
            rows: vec![vec![Value::Bool(true)]],
        }
    );
}

fn seeded_scores() -> Database {
    let mut db = Database::new();
    db.execute("CREATE TABLE scores (id INT PRIMARY KEY, points INT, active BOOL);")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (1, 10, true);")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (2, 20, false);")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (3, 30, true);")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (4, 40, true);")
        .unwrap();
    db
}
