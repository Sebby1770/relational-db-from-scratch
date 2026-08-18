use relational_db_from_scratch::{Database, DbError, QueryResult, Value};

#[test]
fn having_filters_groups_after_aggregation() {
    let mut db = seeded_emp();

    let result = db
        .execute("SELECT dept, COUNT(*) FROM emp GROUP BY dept HAVING COUNT(*) > 1 ORDER BY dept;")
        .unwrap();

    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["dept".into(), "count".into()],
            rows: vec![
                vec![Value::Text("eng".into()), Value::Int(3)],
                vec![Value::Text("hr".into()), Value::Int(2)],
            ],
        }
    );

    let by_sum = db
        .execute("SELECT dept, SUM(n) FROM emp GROUP BY dept HAVING SUM(n) >= 15 ORDER BY dept;")
        .unwrap();
    assert_eq!(
        by_sum,
        QueryResult::Rows {
            columns: vec!["dept".into(), "sum".into()],
            rows: vec![vec![Value::Text("eng".into()), Value::Int(30)]],
        }
    );
}

#[test]
fn having_explain_mentions_hash_group_and_having() {
    let mut db = seeded_emp();
    let plan = db
        .execute("EXPLAIN SELECT dept, COUNT(*) FROM emp GROUP BY dept HAVING COUNT(*) > 1;")
        .unwrap();

    match plan {
        QueryResult::Plan { plan } => {
            let lower = plan.to_ascii_lowercase();
            assert!(lower.contains("hash group by"), "{plan}");
            assert!(lower.contains("having"), "{plan}");
            assert!(lower.contains("count(*)"), "{plan}");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn distinct_drops_duplicate_projected_rows() {
    let mut db = seeded_emp();

    let single = db
        .execute("SELECT DISTINCT dept FROM emp ORDER BY dept;")
        .unwrap();
    assert_eq!(
        single,
        QueryResult::Rows {
            columns: vec!["dept".into()],
            rows: vec![
                vec![Value::Text("eng".into())],
                vec![Value::Text("hr".into())],
                vec![Value::Text("sales".into())],
            ],
        }
    );

    db.execute("INSERT INTO emp VALUES ('eng', 10);").unwrap();
    let multi = db
        .execute("SELECT DISTINCT dept, n FROM emp ORDER BY dept, n;")
        .unwrap();
    assert_eq!(
        multi,
        QueryResult::Rows {
            columns: vec!["dept".into(), "n".into()],
            rows: vec![
                vec![Value::Text("eng".into()), Value::Int(0)],
                vec![Value::Text("eng".into()), Value::Int(10)],
                vec![Value::Text("eng".into()), Value::Int(20)],
                vec![Value::Text("hr".into()), Value::Int(5)],
                vec![Value::Text("hr".into()), Value::Int(7)],
                vec![Value::Text("sales".into()), Value::Int(3)],
            ],
        }
    );
}

#[test]
fn insert_select_copies_filtered_and_joined_rows() {
    let mut db = Database::new();
    db.execute("CREATE TABLE src (id INT, name TEXT);").unwrap();
    db.execute("CREATE TABLE dest (id INT, name TEXT);")
        .unwrap();
    db.execute("CREATE TABLE extras (user_id INT, tag TEXT);")
        .unwrap();
    db.execute("INSERT INTO src VALUES (1, 'Ada');").unwrap();
    db.execute("INSERT INTO src VALUES (2, 'Grace');").unwrap();
    db.execute("INSERT INTO src VALUES (3, 'Alan');").unwrap();
    db.execute("INSERT INTO extras VALUES (1, 'ok');").unwrap();
    db.execute("INSERT INTO extras VALUES (2, 'skip');")
        .unwrap();

    let inserted = db
        .execute("INSERT INTO dest SELECT id, name FROM src WHERE id <= 2;")
        .unwrap();
    assert_eq!(inserted, QueryResult::RowsInserted { count: 2 });

    let copied = db
        .execute("SELECT id, name FROM dest ORDER BY id;")
        .unwrap();
    assert_eq!(
        copied,
        QueryResult::Rows {
            columns: vec!["id".into(), "name".into()],
            rows: vec![
                vec![Value::Int(1), Value::Text("Ada".into())],
                vec![Value::Int(2), Value::Text("Grace".into())],
            ],
        }
    );

    db.execute("CREATE TABLE joined (name TEXT, tag TEXT);")
        .unwrap();
    let joined = db
        .execute(
            "INSERT INTO joined \
             SELECT src.name, extras.tag \
             FROM src JOIN extras ON src.id = extras.user_id \
             WHERE extras.tag = 'ok';",
        )
        .unwrap();
    assert_eq!(joined, QueryResult::RowsInserted { count: 1 });

    let result = db.execute("SELECT name, tag FROM joined;").unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["name".into(), "tag".into()],
            rows: vec![vec![Value::Text("Ada".into()), Value::Text("ok".into())]],
        }
    );
}

#[test]
fn insert_select_rejects_column_count_mismatch() {
    let mut db = Database::new();
    db.execute("CREATE TABLE src (id INT, name TEXT);").unwrap();
    db.execute("CREATE TABLE dest (id INT, name TEXT, extra INT);")
        .unwrap();
    db.execute("INSERT INTO src VALUES (1, 'Ada');").unwrap();

    let error = db
        .execute("INSERT INTO dest SELECT id, name FROM src;")
        .unwrap_err();
    assert!(matches!(
        error,
        DbError::ArityMismatch {
            expected: 3,
            got: 2
        }
    ));
}

#[test]
fn min_max_avg_compute_grouped_values() {
    let mut db = seeded_emp();

    let result = db
        .execute("SELECT dept, MIN(n), MAX(n), AVG(n) FROM emp GROUP BY dept ORDER BY dept;")
        .unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["dept".into(), "min".into(), "max".into(), "avg".into()],
            rows: vec![
                vec![
                    Value::Text("eng".into()),
                    Value::Int(0),
                    Value::Int(20),
                    Value::Int(10),
                ],
                vec![
                    Value::Text("hr".into()),
                    Value::Int(5),
                    Value::Int(7),
                    Value::Int(6),
                ],
                vec![
                    Value::Text("sales".into()),
                    Value::Int(3),
                    Value::Int(3),
                    Value::Int(3),
                ],
            ],
        }
    );

    // 10+20+5+7+3+0 = 45, 45/6 = 7 truncated INT
    let global = db
        .execute("SELECT MIN(n), MAX(n), AVG(n) FROM emp;")
        .unwrap();
    assert_eq!(
        global,
        QueryResult::Rows {
            columns: vec!["min".into(), "max".into(), "avg".into()],
            rows: vec![vec![Value::Int(0), Value::Int(20), Value::Int(7)]],
        }
    );
}

#[test]
fn between_and_in_filter_rows() {
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

    let between = db
        .execute("SELECT id FROM scores WHERE points BETWEEN 20 AND 30 ORDER BY id;")
        .unwrap();
    assert_eq!(
        between,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(2)], vec![Value::Int(3)]],
        }
    );

    let in_list = db
        .execute("SELECT id FROM scores WHERE grade IN ('A', 'C') ORDER BY id;")
        .unwrap();
    assert_eq!(
        in_list,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![
                vec![Value::Int(1)],
                vec![Value::Int(3)],
                vec![Value::Int(4)]
            ],
        }
    );

    let combined = db
        .execute(
            "SELECT id FROM scores WHERE points IN (10, 40) AND id BETWEEN 1 AND 4 ORDER BY id;",
        )
        .unwrap();
    assert_eq!(
        combined,
        QueryResult::Rows {
            columns: vec!["id".into()],
            rows: vec![vec![Value::Int(1)], vec![Value::Int(4)]],
        }
    );
}

fn seeded_emp() -> Database {
    let mut db = Database::new();
    db.execute("CREATE TABLE emp (dept TEXT, n INT);").unwrap();
    db.execute("INSERT INTO emp VALUES ('eng', 10);").unwrap();
    db.execute("INSERT INTO emp VALUES ('eng', 20);").unwrap();
    db.execute("INSERT INTO emp VALUES ('hr', 5);").unwrap();
    db.execute("INSERT INTO emp VALUES ('hr', 7);").unwrap();
    db.execute("INSERT INTO emp VALUES ('sales', 3);").unwrap();
    db.execute("INSERT INTO emp VALUES ('eng', 0);").unwrap();
    db
}
