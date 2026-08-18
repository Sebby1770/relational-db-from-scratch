use relational_db_from_scratch::{Database, QueryResult, Value};

#[test]
fn except_and_intersect_are_set_operations() {
    let mut db = Database::new();
    db.execute("CREATE TABLE left_t (n INT);").unwrap();
    db.execute("CREATE TABLE right_t (n INT);").unwrap();
    db.execute("INSERT INTO left_t VALUES (1), (2), (2), (3);")
        .unwrap();
    db.execute("INSERT INTO right_t VALUES (2), (4);").unwrap();

    let excepted = db
        .execute("SELECT n FROM left_t EXCEPT SELECT n FROM right_t ORDER BY n;")
        .unwrap();
    assert_eq!(
        excepted,
        QueryResult::Rows {
            columns: vec!["n".into()],
            rows: vec![vec![Value::Int(1)], vec![Value::Int(3)]],
        }
    );

    let intersected = db
        .execute("SELECT n FROM left_t INTERSECT SELECT n FROM right_t ORDER BY n;")
        .unwrap();
    assert_eq!(
        intersected,
        QueryResult::Rows {
            columns: vec!["n".into()],
            rows: vec![vec![Value::Int(2)]],
        }
    );
}

#[test]
fn cross_join_is_cartesian_and_right_join_pads_left() {
    let mut db = Database::new();
    db.execute("CREATE TABLE colors (c TEXT);").unwrap();
    db.execute("CREATE TABLE sizes (s TEXT);").unwrap();
    db.execute("INSERT INTO colors VALUES ('red'), ('blue');")
        .unwrap();
    db.execute("INSERT INTO sizes VALUES ('S'), ('L');")
        .unwrap();

    let crossed = db
        .execute("SELECT c, s FROM colors CROSS JOIN sizes ORDER BY c, s;")
        .unwrap();
    assert_eq!(
        crossed,
        QueryResult::Rows {
            columns: vec!["c".into(), "s".into()],
            rows: vec![
                vec![Value::Text("blue".into()), Value::Text("L".into())],
                vec![Value::Text("blue".into()), Value::Text("S".into())],
                vec![Value::Text("red".into()), Value::Text("L".into())],
                vec![Value::Text("red".into()), Value::Text("S".into())],
            ],
        }
    );

    db.execute("CREATE TABLE owners (id INT, name TEXT);")
        .unwrap();
    db.execute("CREATE TABLE pets (id INT, owner_id INT);")
        .unwrap();
    db.execute("INSERT INTO owners VALUES (1, 'Ada'), (2, 'Grace');")
        .unwrap();
    db.execute("INSERT INTO pets VALUES (10, 1), (11, 3);")
        .unwrap();

    let right = db
        .execute(
            "SELECT owners.name, pets.id FROM owners RIGHT JOIN pets ON owners.id = pets.owner_id ORDER BY pets.id;",
        )
        .unwrap();
    assert_eq!(
        right,
        QueryResult::Rows {
            columns: vec!["owners.name".into(), "pets.id".into()],
            rows: vec![
                vec![Value::Text("Ada".into()), Value::Int(10)],
                vec![Value::Null, Value::Int(11)],
            ],
        }
    );
}

#[test]
fn create_table_as_select_and_if_not_exists() {
    let mut db = Database::new();
    db.execute("CREATE TABLE src (id INT, label TEXT);")
        .unwrap();
    db.execute("INSERT INTO src VALUES (1, 'a'), (2, 'b');")
        .unwrap();

    let created = db
        .execute("CREATE TABLE dest AS SELECT id, label FROM src;")
        .unwrap();
    assert_eq!(
        created,
        QueryResult::TableCreated {
            table: "dest".into()
        }
    );

    let copied = db
        .execute("SELECT id, label FROM dest ORDER BY id;")
        .unwrap();
    assert_eq!(
        copied,
        QueryResult::Rows {
            columns: vec!["id".into(), "label".into()],
            rows: vec![
                vec![Value::Int(1), Value::Text("a".into())],
                vec![Value::Int(2), Value::Text("b".into())],
            ],
        }
    );

    let again = db
        .execute("CREATE TABLE IF NOT EXISTS dest AS SELECT id FROM src;")
        .unwrap();
    assert_eq!(
        again,
        QueryResult::TableCreated {
            table: "dest".into()
        }
    );
    let count = db.execute("SELECT COUNT(*) FROM dest;").unwrap();
    assert_eq!(
        count,
        QueryResult::Rows {
            columns: vec!["count".into()],
            rows: vec![vec![Value::Int(2)]],
        }
    );
}

#[test]
fn truncate_rename_coalesce_and_order_by_position() {
    let mut db = Database::new();
    db.execute("CREATE TABLE items (id INT, label TEXT);")
        .unwrap();
    db.execute("INSERT INTO items VALUES (1, NULL), (2, 'kept');")
        .unwrap();

    let coalesced = db
        .execute("SELECT id, COALESCE(label, 'missing') FROM items ORDER BY 1;")
        .unwrap();
    assert_eq!(
        coalesced,
        QueryResult::Rows {
            columns: vec!["id".into(), "coalesce".into()],
            rows: vec![
                vec![Value::Int(1), Value::Text("missing".into())],
                vec![Value::Int(2), Value::Text("kept".into())],
            ],
        }
    );

    let renamed = db.execute("ALTER TABLE items RENAME TO stuff;").unwrap();
    assert_eq!(
        renamed,
        QueryResult::TableRenamed {
            from: "items".into(),
            to: "stuff".into(),
        }
    );

    let truncated = db.execute("TRUNCATE TABLE stuff;").unwrap();
    assert_eq!(truncated, QueryResult::RowsDeleted { count: 2 });
    let empty = db.execute("SELECT COUNT(*) FROM stuff;").unwrap();
    assert_eq!(
        empty,
        QueryResult::Rows {
            columns: vec!["count".into()],
            rows: vec![vec![Value::Int(0)]],
        }
    );

    let dropped = db.execute("DROP TABLE IF EXISTS missing_table;").unwrap();
    assert_eq!(
        dropped,
        QueryResult::TableDropped {
            table: "missing_table".into()
        }
    );
}

#[test]
fn hash_join_explain_and_inner_join_still_matches() {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT, name TEXT);")
        .unwrap();
    db.execute("CREATE TABLE orders (id INT, user_id INT, total INT);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'Ada'), (2, 'Grace');")
        .unwrap();
    db.execute("INSERT INTO orders VALUES (10, 1, 100), (11, 1, 250);")
        .unwrap();

    let plan = db
        .execute("EXPLAIN SELECT users.name, orders.total FROM users JOIN orders ON users.id = orders.user_id;")
        .unwrap();
    match plan {
        QueryResult::Plan { plan } => {
            assert!(plan.to_ascii_lowercase().contains("hash join"));
        }
        other => panic!("unexpected plan: {other:?}"),
    }

    let rows = db
        .execute(
            "SELECT name, total FROM users JOIN orders ON users.id = orders.user_id ORDER BY total;",
        )
        .unwrap();
    assert_eq!(
        rows,
        QueryResult::Rows {
            columns: vec!["name".into(), "total".into()],
            rows: vec![
                vec![Value::Text("Ada".into()), Value::Int(100)],
                vec![Value::Text("Ada".into()), Value::Int(250)],
            ],
        }
    );
}

#[test]
fn rename_rolls_back_inside_transaction() {
    let mut db = Database::new();
    db.execute("CREATE TABLE items (id INT);").unwrap();
    db.execute("INSERT INTO items VALUES (1);").unwrap();
    db.execute("BEGIN;").unwrap();
    db.execute("ALTER TABLE items RENAME TO stuff;").unwrap();
    db.execute("ROLLBACK;").unwrap();
    let still_there = db.execute("SELECT id FROM items;").unwrap();
    assert_eq!(
        still_there,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(1)]],
        }
    );
}
