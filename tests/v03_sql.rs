use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use relational_db_from_scratch::{Database, QueryResult, Value};

#[test]
fn inner_join_matches_related_rows_and_applies_filter() {
    let mut db = seeded_orders();

    let result = db
        .execute(
            "SELECT users.name, orders.total \
             FROM users \
             JOIN orders ON users.id = orders.user_id \
             WHERE orders.total > 80 \
             ORDER BY orders.total;",
        )
        .unwrap();

    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["users.name".into(), "orders.total".into()],
            rows: vec![
                vec![Value::Text("Ada".into()), Value::Int(100)],
                vec![Value::Text("Ada".into()), Value::Int(250)],
            ],
        }
    );
}

#[test]
fn inner_join_explain_uses_nested_loop_join() {
    let mut db = seeded_orders();
    let plan = db
        .execute("EXPLAIN SELECT users.name, orders.total FROM users JOIN orders ON users.id = orders.user_id;")
        .unwrap();

    assert!(
        matches!(plan, QueryResult::Plan { ref plan } if plan.to_ascii_lowercase().contains("nested loop join"))
    );
}

#[test]
fn join_uses_unambiguous_short_names_and_qualified_star() {
    let mut db = seeded_orders();

    let short = db
        .execute("SELECT name, total FROM users JOIN orders ON users.id = orders.user_id ORDER BY total;")
        .unwrap();
    match short {
        QueryResult::Rows { columns, rows } => {
            assert_eq!(columns, vec!["name", "total"]);
            assert_eq!(rows.len(), 3);
        }
        other => panic!("unexpected result: {other:?}"),
    }

    let star = db
        .execute("SELECT * FROM users JOIN orders ON users.id = orders.user_id;")
        .unwrap();
    match star {
        QueryResult::Rows { columns, .. } => {
            assert!(columns.contains(&"users.id".to_string()));
            assert!(columns.contains(&"orders.id".to_string()));
            assert!(columns.contains(&"name".to_string()));
            assert!(columns.contains(&"total".to_string()));
        }
        other => panic!("unexpected result: {other:?}"),
    }

    assert!(
        db.execute("SELECT id FROM users JOIN orders ON users.id = orders.user_id;")
            .is_err()
    );
}

#[test]
fn left_join_null_extends_unmatched_right_rows() {
    let mut db = seeded_orders();
    let result = db
        .execute(
            "SELECT users.name, orders.total \
             FROM users \
             LEFT JOIN orders ON users.id = orders.user_id \
             ORDER BY users.name, orders.total;",
        )
        .unwrap();

    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["users.name".into(), "orders.total".into()],
            rows: vec![
                vec![Value::Text("Ada".into()), Value::Int(100)],
                vec![Value::Text("Ada".into()), Value::Int(250)],
                vec![Value::Text("Alan".into()), Value::Null],
                vec![Value::Text("Grace".into()), Value::Int(50)],
            ],
        }
    );
}

#[test]
fn group_by_counts_and_sums() {
    let mut db = Database::new();
    db.execute("CREATE TABLE emp (dept TEXT, n INT);").unwrap();
    db.execute("INSERT INTO emp VALUES ('eng', 10);").unwrap();
    db.execute("INSERT INTO emp VALUES ('eng', 20);").unwrap();
    db.execute("INSERT INTO emp VALUES ('hr', 5);").unwrap();
    db.execute("INSERT INTO emp VALUES ('hr', 7);").unwrap();
    db.execute("INSERT INTO emp VALUES ('sales', 3);").unwrap();

    let result = db
        .execute("SELECT dept, COUNT(*), SUM(n) FROM emp GROUP BY dept ORDER BY dept;")
        .unwrap();

    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["dept".into(), "count".into(), "sum".into()],
            rows: vec![
                vec![Value::Text("eng".into()), Value::Int(2), Value::Int(30)],
                vec![Value::Text("hr".into()), Value::Int(2), Value::Int(12)],
                vec![Value::Text("sales".into()), Value::Int(1), Value::Int(3)],
            ],
        }
    );

    let global = db.execute("SELECT COUNT(*), SUM(n) FROM emp;").unwrap();
    assert_eq!(
        global,
        QueryResult::Rows {
            columns: vec!["count".into(), "sum".into()],
            rows: vec![vec![Value::Int(5), Value::Int(45)]],
        }
    );
}

#[test]
fn like_matches_prefix_suffix_and_contains() {
    let mut db = Database::new();
    db.execute("CREATE TABLE words (id INT, word TEXT);")
        .unwrap();
    db.execute("INSERT INTO words VALUES (1, 'foobar');")
        .unwrap();
    db.execute("INSERT INTO words VALUES (2, 'barfoo');")
        .unwrap();
    db.execute("INSERT INTO words VALUES (3, 'midbarend');")
        .unwrap();
    db.execute("INSERT INTO words VALUES (4, 'other');")
        .unwrap();
    db.execute("INSERT INTO words VALUES (5, '100% off');")
        .unwrap();

    assert_eq!(
        ids(
            &mut db,
            "SELECT id FROM words WHERE word LIKE 'foo%' ORDER BY id;"
        ),
        vec![1]
    );
    assert_eq!(
        ids(
            &mut db,
            "SELECT id FROM words WHERE word LIKE '%foo' ORDER BY id;"
        ),
        vec![2]
    );
    assert_eq!(
        ids(
            &mut db,
            "SELECT id FROM words WHERE word LIKE '%bar%' ORDER BY id;"
        ),
        vec![1, 2, 3]
    );
    assert_eq!(
        ids(
            &mut db,
            "SELECT id FROM words WHERE word LIKE '%mid%' ORDER BY id;"
        ),
        vec![3]
    );
    assert_eq!(
        ids(
            &mut db,
            "SELECT id FROM words WHERE word LIKE '100\\% off' ESCAPE '\\' ORDER BY id;"
        ),
        vec![5]
    );
}

#[test]
fn limit_offset_applies_after_order_by() {
    let mut db = Database::new();
    db.execute("CREATE TABLE scores (id INT PRIMARY KEY, points INT);")
        .unwrap();
    for id in 1..=5 {
        db.execute(&format!("INSERT INTO scores VALUES ({id}, {});", id * 10))
            .unwrap();
    }

    let result = db
        .execute("SELECT id FROM scores ORDER BY id LIMIT 2 OFFSET 1;")
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(2)], vec![Value::Int(3)]],
        }
    );

    let tail = db
        .execute("SELECT id FROM scores ORDER BY id DESC LIMIT 1 OFFSET 4;")
        .unwrap();
    assert_eq!(
        tail,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(1)]],
        }
    );
}

#[test]
fn copy_from_csv_imports_header_columns() {
    let temp = std::env::temp_dir().join(format!(
        "rdb-import-{}-{}.csv",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(
        &temp,
        "id,name,active\n1,Ada Lovelace,true\n2,Grace Hopper,false\n",
    )
    .unwrap();

    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT, name TEXT, active BOOL);")
        .unwrap();
    let imported = db
        .execute(&format!("COPY users FROM '{}';", temp.display()))
        .unwrap();
    assert_eq!(imported, QueryResult::RowsInserted { count: 2 });

    let result = db
        .execute("SELECT id, name, active FROM users ORDER BY id;")
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into(), "name".into(), "active".into()],
            rows: vec![
                vec![
                    Value::Int(1),
                    Value::Text("Ada Lovelace".into()),
                    Value::Bool(true)
                ],
                vec![
                    Value::Int(2),
                    Value::Text("Grace Hopper".into()),
                    Value::Bool(false)
                ],
            ],
        }
    );

    let _ = fs::remove_file(temp);
}

#[test]
fn import_csv_helper_uses_header_mapping() {
    let temp = std::env::temp_dir().join(format!(
        "rdb-import-helper-{}-{}.csv",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&temp, "name,id\nAda,1\nGrace,2\n").unwrap();

    let mut db = Database::new();
    db.execute("CREATE TABLE people (id INT, name TEXT);")
        .unwrap();
    assert_eq!(
        db.import_csv(&temp, "people").unwrap(),
        QueryResult::RowsInserted { count: 2 }
    );

    let result = db
        .execute("SELECT id, name FROM people ORDER BY id;")
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into(), "name".into()],
            rows: vec![
                vec![Value::Int(1), Value::Text("Ada".into())],
                vec![Value::Int(2), Value::Text("Grace".into())],
            ],
        }
    );

    let _ = fs::remove_file(temp);
}

fn ids(db: &mut Database, sql: &str) -> Vec<i64> {
    match db.execute(sql).unwrap() {
        QueryResult::Rows { rows, .. } => rows
            .into_iter()
            .map(|row| match row[0] {
                Value::Int(value) => value,
                _ => panic!("expected int id"),
            })
            .collect(),
        other => panic!("unexpected result: {other:?}"),
    }
}

fn seeded_orders() -> Database {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute("CREATE TABLE orders (id INT PRIMARY KEY, user_id INT, total INT);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'Ada');").unwrap();
    db.execute("INSERT INTO users VALUES (2, 'Grace');")
        .unwrap();
    db.execute("INSERT INTO users VALUES (3, 'Alan');").unwrap();
    db.execute("INSERT INTO orders VALUES (10, 1, 100);")
        .unwrap();
    db.execute("INSERT INTO orders VALUES (11, 1, 250);")
        .unwrap();
    db.execute("INSERT INTO orders VALUES (12, 2, 50);")
        .unwrap();
    db
}
