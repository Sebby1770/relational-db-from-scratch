use relational_db_from_scratch::{Database, DbError, QueryResult, Value};

#[test]
fn create_insert_and_select_all_rows() {
    let mut db = Database::new();

    assert!(
        db.execute("CREATE TABLE users (id INT, name TEXT, active BOOL);")
            .is_ok()
    );
    assert!(
        db.execute("INSERT INTO users VALUES (1, 'Ada Lovelace', true);")
            .is_ok()
    );
    assert!(
        db.execute("INSERT INTO users VALUES (2, 'Grace Hopper', false);")
            .is_ok()
    );

    let result = db.execute("SELECT * FROM users;").unwrap();

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
}

#[test]
fn select_projected_columns_with_where_predicate() {
    let mut db = seeded_database();

    let result = db
        .execute("SELECT name, active FROM users WHERE id = 2;")
        .unwrap();

    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["name".into(), "active".into()],
            rows: vec![vec![Value::Text("Grace Hopper".into()), Value::Bool(false)]],
        }
    );
}

#[test]
fn update_and_delete_respect_predicates() {
    let mut db = seeded_database();

    assert_eq!(
        db.execute("UPDATE users SET active = true WHERE id = 2;")
            .unwrap(),
        QueryResult::RowsUpdated { count: 1 }
    );
    assert_eq!(
        db.execute("DELETE FROM users WHERE name = 'Ada Lovelace';")
            .unwrap(),
        QueryResult::RowsDeleted { count: 1 }
    );

    let result = db.execute("SELECT id, active FROM users;").unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["id".into(), "active".into()],
            rows: vec![vec![Value::Int(2), Value::Bool(true)]],
        }
    );
}

#[test]
fn rejects_type_mismatches() {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT, name TEXT, active BOOL);")
        .unwrap();

    let error = db
        .execute("INSERT INTO users VALUES ('not an int', 'Ada', true);")
        .unwrap_err();

    assert_eq!(
        error,
        DbError::TypeMismatch {
            column: "id".into(),
            expected: "INT".into(),
            got: "TEXT".into()
        }
    );
}

#[test]
fn rejects_duplicate_columns() {
    let mut db = Database::new();

    let error = db
        .execute("CREATE TABLE broken (id INT, id TEXT);")
        .unwrap_err();

    assert_eq!(error, DbError::ColumnExists("id".into()));
}

#[test]
fn parses_escaped_single_quotes() {
    let mut db = Database::new();
    db.execute("CREATE TABLE notes (id INT, body TEXT);")
        .unwrap();
    db.execute("INSERT INTO notes VALUES (1, 'Ada''s note');")
        .unwrap();

    let result = db.execute("SELECT body FROM notes WHERE id = 1;").unwrap();

    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["body".into()],
            rows: vec![vec![Value::Text("Ada's note".into())]],
        }
    );
}

fn seeded_database() -> Database {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT, name TEXT, active BOOL);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'Ada Lovelace', true);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (2, 'Grace Hopper', false);")
        .unwrap();
    db
}
