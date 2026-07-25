//! Joins: INNER, LEFT OUTER, CROSS, aliases, and qualified column names.
//!
//! The interesting cases are the ones where a join is not just a filtered
//! product: NULL-extension of unmatched left rows, NULL keys never matching
//! (even against another NULL), ambiguous unqualified column references, and
//! joins feeding the aggregation path.

use relational_db_from_scratch::{Database, QueryResult, Value};

fn rows(result: QueryResult) -> (Vec<String>, Vec<Vec<Value>>) {
    match result {
        QueryResult::Rows { columns, rows } => (columns, rows),
        other => panic!("expected rows, got {other:?}"),
    }
}

fn plan(result: QueryResult) -> String {
    match result {
        QueryResult::Plan { plan } => plan,
        other => panic!("expected a plan, got {other:?}"),
    }
}

/// users 1..3, where user 3 has no orders and one order points at a user that
/// does not exist — so both sides have unmatched rows.
fn seeded() -> Database {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute("CREATE TABLE orders (id INT PRIMARY KEY, user_id INT, amount INT);")
        .unwrap();
    for (id, name) in [(1, "ada"), (2, "grace"), (3, "lonely")] {
        db.execute(&format!("INSERT INTO users VALUES ({id}, '{name}');"))
            .unwrap();
    }
    for (id, user, amount) in [(10, "1", "100"), (11, "1", "250"), (12, "2", "70")] {
        db.execute(&format!(
            "INSERT INTO orders VALUES ({id}, {user}, {amount});"
        ))
        .unwrap();
    }
    db
}

#[test]
fn inner_join_pairs_matching_rows_only() {
    let mut db = seeded();
    let (columns, data) = rows(
        db.execute(
            "SELECT users.name, orders.amount FROM users \
             JOIN orders ON users.id = orders.user_id ORDER BY orders.amount;",
        )
        .unwrap(),
    );

    assert_eq!(columns, vec!["users.name", "orders.amount"]);
    // 'lonely' has no orders, so an inner join drops it entirely.
    assert_eq!(
        data,
        vec![
            vec![Value::Text("grace".into()), Value::Int(70)],
            vec![Value::Text("ada".into()), Value::Int(100)],
            vec![Value::Text("ada".into()), Value::Int(250)],
        ]
    );
}

#[test]
fn left_join_null_extends_unmatched_rows() {
    let mut db = seeded();
    let (_columns, data) = rows(
        db.execute(
            "SELECT users.name, orders.amount FROM users \
             LEFT JOIN orders ON users.id = orders.user_id ORDER BY users.name;",
        )
        .unwrap(),
    );

    // 'lonely' survives with a NULL right-hand side.
    let lonely = data
        .iter()
        .find(|row| row[0] == Value::Text("lonely".into()))
        .expect("unmatched left row is kept");
    assert_eq!(lonely[1], Value::Null, "right side is NULL-extended");
    assert_eq!(data.len(), 4, "3 matches plus the unmatched row");
}

#[test]
fn left_outer_join_is_the_same_as_left_join() {
    let mut db = seeded();
    let a = rows(
        db.execute("SELECT users.name FROM users LEFT JOIN orders ON users.id = orders.user_id;")
            .unwrap(),
    );
    let b = rows(
        db.execute(
            "SELECT users.name FROM users LEFT OUTER JOIN orders ON users.id = orders.user_id;",
        )
        .unwrap(),
    );
    assert_eq!(a, b, "OUTER is optional noise");
}

#[test]
fn cross_join_produces_the_full_product() {
    let mut db = seeded();
    let (_columns, data) = rows(
        db.execute("SELECT users.id, orders.id FROM users CROSS JOIN orders;")
            .unwrap(),
    );
    assert_eq!(data.len(), 9, "3 users x 3 orders");

    // A comma in the FROM list means the same thing.
    let (_c2, comma) = rows(
        db.execute("SELECT users.id, orders.id FROM users, orders;")
            .unwrap(),
    );
    assert_eq!(comma.len(), 9, "comma join is a cross join");
}

#[test]
fn aliases_work_and_qualify_columns() {
    let mut db = seeded();
    let (columns, data) = rows(
        db.execute(
            "SELECT u.name, o.amount FROM users u JOIN orders AS o \
             ON u.id = o.user_id ORDER BY o.amount;",
        )
        .unwrap(),
    );
    assert_eq!(columns, vec!["u.name", "o.amount"]);
    assert_eq!(data.len(), 3);
    assert_eq!(data[0][0], Value::Text("grace".into()));
}

#[test]
fn unqualified_columns_resolve_when_unambiguous() {
    let mut db = seeded();
    // `name` and `amount` each exist in only one of the two tables.
    let (_columns, data) = rows(
        db.execute(
            "SELECT name, amount FROM users JOIN orders ON users.id = orders.user_id \
             ORDER BY amount;",
        )
        .unwrap(),
    );
    assert_eq!(data.len(), 3);
    assert_eq!(data[0][1], Value::Int(70));
}

#[test]
fn ambiguous_unqualified_column_is_rejected() {
    let mut db = seeded();
    // `id` exists in both tables, so a bare `id` cannot be resolved.
    let error = db
        .execute("SELECT id FROM users JOIN orders ON users.id = orders.user_id;")
        .unwrap_err();
    assert!(
        error.to_string().contains("ambiguous"),
        "expected an ambiguity error, got: {error}"
    );
}

#[test]
fn join_condition_may_be_written_in_either_direction() {
    let mut db = seeded();
    let a = rows(
        db.execute("SELECT users.name FROM users JOIN orders ON users.id = orders.user_id;")
            .unwrap(),
    );
    let b = rows(
        db.execute("SELECT users.name FROM users JOIN orders ON orders.user_id = users.id;")
            .unwrap(),
    );
    assert_eq!(a, b, "the ON clause is symmetric");
}

#[test]
fn null_join_keys_never_match() {
    let mut db = Database::new();
    db.execute("CREATE TABLE a (id INT PRIMARY KEY, k INT);")
        .unwrap();
    db.execute("CREATE TABLE b (id INT PRIMARY KEY, k INT);")
        .unwrap();
    db.execute("INSERT INTO a VALUES (1, null);").unwrap();
    db.execute("INSERT INTO b VALUES (2, null);").unwrap();

    let (_columns, inner) = rows(
        db.execute("SELECT a.id FROM a JOIN b ON a.k = b.k;")
            .unwrap(),
    );
    assert!(
        inner.is_empty(),
        "NULL = NULL is unknown, so the rows must not join"
    );

    // The same row must still survive a LEFT JOIN, NULL-extended.
    let (_c, left) = rows(
        db.execute("SELECT a.id, b.id FROM a LEFT JOIN b ON a.k = b.k;")
            .unwrap(),
    );
    assert_eq!(left, vec![vec![Value::Int(1), Value::Null]]);
}

#[test]
fn non_equality_join_uses_nested_loops() {
    let mut db = seeded();
    let text = plan(
        db.execute("EXPLAIN SELECT users.name FROM users JOIN orders ON users.id < orders.amount;")
            .unwrap(),
    );
    assert!(
        text.contains("NestedLoopJoin"),
        "an inequality join cannot be hashed: {text}"
    );

    let (_columns, data) = rows(
        db.execute("SELECT users.name FROM users JOIN orders ON users.id < orders.amount;")
            .unwrap(),
    );
    // Every user id (1,2,3) is below every amount (70,100,250).
    assert_eq!(data.len(), 9);
}

#[test]
fn equality_join_uses_a_hash_join() {
    let mut db = seeded();
    let text = plan(
        db.execute(
            "EXPLAIN SELECT users.name FROM users JOIN orders ON users.id = orders.user_id;",
        )
        .unwrap(),
    );
    assert!(text.contains("HashJoin"), "equi-join should hash: {text}");
    assert!(text.contains("INNER orders"), "plan names the joined table");
}

#[test]
fn where_clause_filters_the_joined_rows() {
    let mut db = seeded();
    let (_columns, data) = rows(
        db.execute(
            "SELECT users.name, orders.amount FROM users \
             JOIN orders ON users.id = orders.user_id WHERE orders.amount > 90 \
             ORDER BY orders.amount;",
        )
        .unwrap(),
    );
    assert_eq!(
        data,
        vec![
            vec![Value::Text("ada".into()), Value::Int(100)],
            vec![Value::Text("ada".into()), Value::Int(250)],
        ]
    );
}

#[test]
fn aggregation_runs_over_a_join() {
    let mut db = seeded();
    let (columns, data) = rows(
        db.execute(
            "SELECT u.name, COUNT(o.id), SUM(o.amount) FROM users u \
             LEFT JOIN orders o ON u.id = o.user_id GROUP BY u.name ORDER BY u.name;",
        )
        .unwrap(),
    );
    assert_eq!(columns, vec!["u.name", "count_o.id", "sum_o.amount"]);
    assert_eq!(
        data,
        vec![
            vec![Value::Text("ada".into()), Value::Int(2), Value::Int(350)],
            vec![Value::Text("grace".into()), Value::Int(1), Value::Int(70)],
            // The NULL-extended row contributes nothing to either aggregate:
            // COUNT(col) skips NULLs and SUM over no values is NULL.
            vec![Value::Text("lonely".into()), Value::Int(0), Value::Null],
        ]
    );
}

#[test]
fn three_way_join() {
    let mut db = seeded();
    db.execute("CREATE TABLE items (id INT PRIMARY KEY, order_id INT, sku TEXT);")
        .unwrap();
    db.execute("INSERT INTO items VALUES (100, 10, 'widget');")
        .unwrap();
    db.execute("INSERT INTO items VALUES (101, 11, 'gadget');")
        .unwrap();

    let (_columns, data) = rows(
        db.execute(
            "SELECT users.name, items.sku FROM users \
             JOIN orders ON users.id = orders.user_id \
             JOIN items ON orders.id = items.order_id ORDER BY items.sku;",
        )
        .unwrap(),
    );
    assert_eq!(
        data,
        vec![
            vec![Value::Text("ada".into()), Value::Text("gadget".into())],
            vec![Value::Text("ada".into()), Value::Text("widget".into())],
        ]
    );
}

#[test]
fn select_star_returns_every_qualified_column() {
    let mut db = seeded();
    let (columns, data) = rows(
        db.execute("SELECT * FROM users JOIN orders ON users.id = orders.user_id;")
            .unwrap(),
    );
    assert_eq!(
        columns,
        vec![
            "users.id",
            "users.name",
            "orders.id",
            "orders.user_id",
            "orders.amount"
        ]
    );
    assert_eq!(data.len(), 3);
    assert_eq!(data[0].len(), 5);
}

#[test]
fn self_join_requires_distinct_aliases() {
    let mut db = seeded();
    // Without an alias the same name would appear twice and be unresolvable.
    let error = db
        .execute("SELECT users.id FROM users JOIN users ON users.id = users.id;")
        .unwrap_err();
    assert!(
        error.to_string().contains("twice") || error.to_string().contains("alias"),
        "expected a duplicate-name error, got: {error}"
    );

    // With aliases it works.
    let (_columns, data) = rows(
        db.execute("SELECT a.name, b.name FROM users a JOIN users b ON a.id = b.id;")
            .unwrap(),
    );
    assert_eq!(data.len(), 3, "each row joins to itself exactly once");
}

#[test]
fn missing_on_clause_is_rejected() {
    let mut db = seeded();
    assert!(
        db.execute("SELECT users.id FROM users JOIN orders;")
            .is_err(),
        "INNER JOIN without ON must be rejected"
    );
    assert!(
        db.execute("SELECT users.id FROM users CROSS JOIN orders ON users.id = orders.id;")
            .is_err(),
        "CROSS JOIN with ON must be rejected"
    );
}

#[test]
fn joining_a_missing_table_is_an_error() {
    let mut db = seeded();
    assert!(
        db.execute("SELECT users.id FROM users JOIN nope ON users.id = nope.id;")
            .is_err()
    );
}

#[test]
fn limit_applies_to_join_output() {
    let mut db = seeded();
    let (_columns, data) = rows(
        db.execute("SELECT users.name FROM users CROSS JOIN orders ORDER BY users.name LIMIT 4;")
            .unwrap(),
    );
    assert_eq!(data.len(), 4);
}

#[test]
fn single_table_queries_are_unaffected() {
    // The no-join path must be byte-for-byte what it always was: unqualified
    // column names, no synthetic schema.
    let mut db = seeded();
    let (columns, data) = rows(
        db.execute("SELECT id, name FROM users ORDER BY id;")
            .unwrap(),
    );
    assert_eq!(columns, vec!["id", "name"]);
    assert_eq!(data[0], vec![Value::Int(1), Value::Text("ada".into())]);
}
