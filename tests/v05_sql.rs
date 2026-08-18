use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use relational_db_from_scratch::{Database, DbError, QueryResult, Value};

#[test]
fn is_null_and_is_not_null_filter_rows() {
    let mut db = Database::new();
    db.execute("CREATE TABLE items (id INT PRIMARY KEY, label TEXT);")
        .unwrap();
    db.execute("INSERT INTO items VALUES (1, 'alpha');")
        .unwrap();
    db.execute("INSERT INTO items VALUES (2, NULL);").unwrap();
    db.execute("INSERT INTO items VALUES (3, 'beta');").unwrap();

    let is_null = db
        .execute("SELECT id FROM items WHERE label IS NULL ORDER BY id;")
        .unwrap();
    assert_eq!(
        is_null,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(2)]],
        }
    );

    let is_not_null = db
        .execute("SELECT id FROM items WHERE label IS NOT NULL ORDER BY id;")
        .unwrap();
    assert_eq!(
        is_not_null,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(1)], vec![Value::Int(3)]],
        }
    );
}

#[test]
fn not_in_and_not_between_filter_rows() {
    let mut db = Database::new();
    db.execute("CREATE TABLE scores (id INT PRIMARY KEY, points INT, grade TEXT);")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (1, 10, 'C');")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (2, 20, 'B');")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (3, 30, 'A');")
        .unwrap();
    db.execute("INSERT INTO scores VALUES (4, 40, 'A');")
        .unwrap();

    let not_between = db
        .execute("SELECT id FROM scores WHERE points NOT BETWEEN 20 AND 30 ORDER BY id;")
        .unwrap();
    assert_eq!(
        not_between,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(1)], vec![Value::Int(4)]],
        }
    );

    let not_in = db
        .execute("SELECT id FROM scores WHERE grade NOT IN ('A', 'C') ORDER BY id;")
        .unwrap();
    assert_eq!(
        not_in,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(2)]],
        }
    );
}

#[test]
fn count_column_skips_nulls_unlike_count_star() {
    let mut db = Database::new();
    db.execute("CREATE TABLE items (id INT, label TEXT);")
        .unwrap();
    db.execute("INSERT INTO items VALUES (1, 'a');").unwrap();
    db.execute("INSERT INTO items VALUES (2, NULL);").unwrap();
    db.execute("INSERT INTO items VALUES (3, 'c');").unwrap();
    db.execute("INSERT INTO items VALUES (NULL, 'd');").unwrap();

    let both = db
        .execute("SELECT COUNT(*), COUNT(id), COUNT(label) FROM items;")
        .unwrap();
    assert_eq!(
        both,
        QueryResult::Rows {
            columns: vec!["count".into(), "count".into(), "count".into()],
            rows: vec![vec![Value::Int(4), Value::Int(3), Value::Int(3)]],
        }
    );
}

#[test]
fn union_dedupes_and_union_all_keeps_duplicates() {
    let mut db = Database::new();
    db.execute("CREATE TABLE left_t (n INT);").unwrap();
    db.execute("CREATE TABLE right_t (n INT);").unwrap();
    db.execute("INSERT INTO left_t VALUES (1);").unwrap();
    db.execute("INSERT INTO left_t VALUES (2);").unwrap();
    db.execute("INSERT INTO left_t VALUES (2);").unwrap();
    db.execute("INSERT INTO right_t VALUES (2);").unwrap();
    db.execute("INSERT INTO right_t VALUES (3);").unwrap();

    let unioned = db
        .execute("SELECT n FROM left_t UNION SELECT n FROM right_t ORDER BY n;")
        .unwrap();
    assert_eq!(
        unioned,
        QueryResult::Rows {
            columns: vec!["n".into()],
            rows: vec![
                vec![Value::Int(1)],
                vec![Value::Int(2)],
                vec![Value::Int(3)]
            ],
        }
    );

    let union_all = db
        .execute("SELECT n FROM left_t UNION ALL SELECT n FROM right_t ORDER BY n;")
        .unwrap();
    assert_eq!(
        union_all,
        QueryResult::Rows {
            columns: vec!["n".into()],
            rows: vec![
                vec![Value::Int(1)],
                vec![Value::Int(2)],
                vec![Value::Int(2)],
                vec![Value::Int(2)],
                vec![Value::Int(3)],
            ],
        }
    );

    db.execute("CREATE TABLE dest (n INT);").unwrap();
    let inserted = db
        .execute("INSERT INTO dest SELECT n FROM left_t UNION SELECT n FROM right_t;")
        .unwrap();
    assert_eq!(inserted, QueryResult::RowsInserted { count: 3 });
}

#[test]
fn union_rejects_column_count_mismatch() {
    let mut db = Database::new();
    db.execute("CREATE TABLE left_t (n INT);").unwrap();
    db.execute("CREATE TABLE right_t (n INT, extra INT);")
        .unwrap();
    db.execute("INSERT INTO left_t VALUES (1);").unwrap();
    db.execute("INSERT INTO right_t VALUES (1, 2);").unwrap();

    let error = db
        .execute("SELECT n FROM left_t UNION SELECT n, extra FROM right_t;")
        .unwrap_err();
    assert!(matches!(
        error,
        DbError::ArityMismatch {
            expected: 1,
            got: 2
        }
    ));
}

#[test]
fn case_when_projects_then_or_else() {
    let mut db = Database::new();
    db.execute("CREATE TABLE emp (name TEXT, n INT);").unwrap();
    db.execute("INSERT INTO emp VALUES ('Ada', 20);").unwrap();
    db.execute("INSERT INTO emp VALUES ('Grace', 5);").unwrap();
    db.execute("INSERT INTO emp VALUES ('Alan', NULL);")
        .unwrap();

    let result = db
        .execute(
            "SELECT name, CASE WHEN n >= 10 THEN 'senior' ELSE 'junior' END \
             FROM emp ORDER BY name;",
        )
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["name".into(), "case".into()],
            rows: vec![
                vec![Value::Text("Ada".into()), Value::Text("senior".into())],
                vec![Value::Text("Alan".into()), Value::Text("junior".into())],
                vec![Value::Text("Grace".into()), Value::Text("junior".into())],
            ],
        }
    );
}

#[test]
fn alter_table_add_column_fills_nulls() {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT, name TEXT);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'Ada');").unwrap();
    db.execute("INSERT INTO users VALUES (2, 'Grace');")
        .unwrap();

    let altered = db
        .execute("ALTER TABLE users ADD COLUMN active BOOL;")
        .unwrap();
    assert_eq!(
        altered,
        QueryResult::ColumnAdded {
            table: "users".into(),
            column: "active".into(),
        }
    );

    let before_insert = db
        .execute("SELECT id, name, active FROM users ORDER BY id;")
        .unwrap();
    assert_eq!(
        before_insert,
        QueryResult::Rows {
            columns: vec!["id".into(), "name".into(), "active".into()],
            rows: vec![
                vec![Value::Int(1), Value::Text("Ada".into()), Value::Null],
                vec![Value::Int(2), Value::Text("Grace".into()), Value::Null],
            ],
        }
    );

    db.execute("INSERT INTO users VALUES (3, 'Alan', true);")
        .unwrap();
    let after_insert = db
        .execute("SELECT id, active FROM users WHERE id = 3;")
        .unwrap();
    assert_eq!(
        after_insert,
        QueryResult::Rows {
            columns: vec!["id".into(), "active".into()],
            rows: vec![vec![Value::Int(3), Value::Bool(true)]],
        }
    );

    let duplicate = db
        .execute("ALTER TABLE users ADD COLUMN name TEXT;")
        .unwrap_err();
    assert!(matches!(duplicate, DbError::ColumnExists(_)));
}

#[test]
fn copy_to_and_export_write_header_and_rows() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let copy_path =
        std::env::temp_dir().join(format!("rdb-export-{}-{}.csv", std::process::id(), stamp));
    let helper_path = std::env::temp_dir().join(format!(
        "rdb-export-helper-{}-{}.csv",
        std::process::id(),
        stamp
    ));

    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT, name TEXT, note TEXT);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'Ada Lovelace', NULL);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (2, 'Hopper, Grace', 'ok');")
        .unwrap();

    let exported = db
        .execute(&format!("COPY users TO '{}';", copy_path.display()))
        .unwrap();
    assert_eq!(
        exported,
        QueryResult::RowsExported {
            path: copy_path.display().to_string(),
            count: 2,
        }
    );

    let content = fs::read_to_string(&copy_path).unwrap();
    assert_eq!(
        content,
        "id,name,note\n1,Ada Lovelace,\n2,\"Hopper, Grace\",ok\n"
    );

    assert_eq!(
        db.export_csv(&helper_path, "users").unwrap(),
        QueryResult::RowsExported {
            path: helper_path.display().to_string(),
            count: 2,
        }
    );
    assert_eq!(
        fs::read_to_string(&helper_path).unwrap(),
        fs::read_to_string(&copy_path).unwrap()
    );

    let _ = fs::remove_file(copy_path);
    let _ = fs::remove_file(helper_path);
}
