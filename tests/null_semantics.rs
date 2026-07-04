use relational_db_from_scratch::{Database, QueryResult, Value};

#[test]
fn null_comparisons_are_unknown_in_where_clauses() {
    let mut db = Database::new();
    db.execute("CREATE TABLE items (id INT PRIMARY KEY, label TEXT);")
        .unwrap();
    db.execute("INSERT INTO items VALUES (1, 'alpha');")
        .unwrap();
    db.execute("INSERT INTO items VALUES (2, NULL);").unwrap();

    let null_eq = db
        .execute("SELECT id FROM items WHERE label = NULL;")
        .unwrap();
    assert_eq!(
        null_eq,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![],
        }
    );

    let null_ne = db
        .execute("SELECT id FROM items WHERE label != NULL;")
        .unwrap();
    assert_eq!(
        null_ne,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![],
        }
    );

    let is_null = db
        .execute("SELECT id FROM items WHERE label = NULL OR id = 2;")
        .unwrap();
    assert_eq!(
        is_null,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(2)]],
        }
    );
}

#[test]
fn analyze_populates_table_statistics() {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'ada@example.com');")
        .unwrap();
    db.execute("INSERT INTO users VALUES (2, 'grace@example.com');")
        .unwrap();
    db.execute("CREATE INDEX users_email_idx ON users(email);")
        .unwrap();
    db.execute("ANALYZE users;").unwrap();

    let plan = db
        .execute("EXPLAIN SELECT id FROM users WHERE email = 'ada@example.com';")
        .unwrap();
    assert!(matches!(plan, QueryResult::Plan { ref plan } if plan.contains("est_rows")));
}

#[test]
fn compound_predicates_can_use_indexes() {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT PRIMARY KEY, email TEXT, active BOOL);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'ada@example.com', true);")
        .unwrap();
    db.execute("CREATE INDEX users_email_idx ON users(email);")
        .unwrap();

    let plan = db
        .execute("EXPLAIN SELECT id FROM users WHERE email = 'ada@example.com' AND active = true;")
        .unwrap();
    assert!(matches!(plan, QueryResult::Plan { ref plan } if plan.contains("IndexLookup")));
}
