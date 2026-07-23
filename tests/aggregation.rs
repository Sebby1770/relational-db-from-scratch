//! Aggregation: GROUP BY, HAVING, and the COUNT/SUM/AVG/MIN/MAX functions.
//!
//! The emphasis here is the SQL NULL rules, which are where aggregate
//! implementations usually go wrong: COUNT(*) counts NULL rows, every other
//! aggregate skips NULLs, and an aggregate over no surviving values is NULL —
//! except COUNT, which is 0.

use relational_db_from_scratch::{Database, QueryResult, Value};

fn rows(result: QueryResult) -> (Vec<String>, Vec<Vec<Value>>) {
    match result {
        QueryResult::Rows { columns, rows } => (columns, rows),
        other => panic!("expected rows, got {other:?}"),
    }
}

/// A sales table with NULLs sprinkled through the amount column so the NULL
/// rules are actually exercised rather than assumed.
fn seeded_sales() -> Database {
    let mut db = Database::new();
    db.execute("CREATE TABLE sales (id INT PRIMARY KEY, region TEXT, amount INT);")
        .unwrap();
    let inserts = [
        "(1, 'north', 100)",
        "(2, 'north', 200)",
        "(3, 'south', 50)",
        "(4, 'south', null)", // a NULL amount in a non-empty group
        "(5, 'east', null)",  // a group whose amounts are ALL null
        "(6, 'north', 300)",
    ];
    for values in inserts {
        db.execute(&format!("INSERT INTO sales VALUES {values};"))
            .unwrap();
    }
    db
}

#[test]
fn group_by_with_count_sum_and_avg() {
    let mut db = seeded_sales();
    let (columns, mut data) = rows(
        db.execute(
            "SELECT region, COUNT(*), SUM(amount), AVG(amount) \
             FROM sales GROUP BY region ORDER BY region;",
        )
        .unwrap(),
    );

    assert_eq!(columns, vec!["region", "count", "sum_amount", "avg_amount"]);
    // ORDER BY region ASC: east, north, south.
    data.sort_by_key(|row| row[0].to_string());
    assert_eq!(
        data,
        vec![
            // east: one row, amount all NULL -> SUM and AVG are NULL, COUNT(*) is 1.
            vec![
                Value::Text("east".into()),
                Value::Int(1),
                Value::Null,
                Value::Null
            ],
            // north: three rows, 100+200+300 = 600, avg 200.
            vec![
                Value::Text("north".into()),
                Value::Int(3),
                Value::Int(600),
                Value::Int(200)
            ],
            // south: two rows but one NULL amount -> SUM 50, AVG over one value 50.
            vec![
                Value::Text("south".into()),
                Value::Int(2),
                Value::Int(50),
                Value::Int(50)
            ],
        ]
    );
}

#[test]
fn count_star_counts_null_rows_but_count_column_does_not() {
    let mut db = seeded_sales();
    let (_columns, data) = rows(
        db.execute("SELECT region, COUNT(*), COUNT(amount) FROM sales GROUP BY region;")
            .unwrap(),
    );

    let south = data
        .iter()
        .find(|row| row[0] == Value::Text("south".into()))
        .expect("south group present");
    // South has two rows, one with a NULL amount.
    assert_eq!(south[1], Value::Int(2), "COUNT(*) includes the NULL row");
    assert_eq!(
        south[2],
        Value::Int(1),
        "COUNT(amount) skips the NULL value"
    );

    let east = data
        .iter()
        .find(|row| row[0] == Value::Text("east".into()))
        .expect("east group present");
    assert_eq!(east[1], Value::Int(1), "COUNT(*) counts the all-NULL row");
    assert_eq!(east[2], Value::Int(0), "COUNT(amount) of all NULLs is 0");
}

#[test]
fn min_and_max_ignore_nulls() {
    let mut db = seeded_sales();
    let (_columns, data) = rows(
        db.execute("SELECT region, MIN(amount), MAX(amount) FROM sales GROUP BY region;")
            .unwrap(),
    );

    let north = data
        .iter()
        .find(|row| row[0] == Value::Text("north".into()))
        .unwrap();
    assert_eq!(north[1], Value::Int(100));
    assert_eq!(north[2], Value::Int(300));

    let east = data
        .iter()
        .find(|row| row[0] == Value::Text("east".into()))
        .unwrap();
    assert_eq!(east[1], Value::Null, "MIN of all NULLs is NULL");
    assert_eq!(east[2], Value::Null, "MAX of all NULLs is NULL");
}

#[test]
fn ungrouped_aggregate_over_whole_table() {
    let mut db = seeded_sales();
    let (columns, data) = rows(
        db.execute("SELECT COUNT(*), SUM(amount), MIN(amount), MAX(amount) FROM sales;")
            .unwrap(),
    );
    assert_eq!(
        columns,
        vec!["count", "sum_amount", "min_amount", "max_amount"]
    );
    assert_eq!(
        data,
        vec![vec![
            Value::Int(6),
            Value::Int(650), // 100+200+50+300
            Value::Int(50),
            Value::Int(300),
        ]]
    );
}

#[test]
fn aggregate_over_empty_table_yields_one_row() {
    let mut db = Database::new();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, v INT);")
        .unwrap();

    let (_columns, data) = rows(
        db.execute("SELECT COUNT(*), SUM(v), AVG(v), MIN(v) FROM t;")
            .unwrap(),
    );
    // SQL: an ungrouped aggregate always produces exactly one row. COUNT is 0;
    // SUM/AVG/MIN over no rows are NULL.
    assert_eq!(
        data,
        vec![vec![Value::Int(0), Value::Null, Value::Null, Value::Null]]
    );
}

#[test]
fn group_by_over_empty_table_yields_no_rows() {
    let mut db = Database::new();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, g TEXT, v INT);")
        .unwrap();
    let (_columns, data) = rows(db.execute("SELECT g, COUNT(*) FROM t GROUP BY g;").unwrap());
    assert!(data.is_empty(), "no rows means no groups");
}

#[test]
fn nulls_form_their_own_group() {
    let mut db = Database::new();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, g TEXT, v INT);")
        .unwrap();
    db.execute("INSERT INTO t VALUES (1, null, 10);").unwrap();
    db.execute("INSERT INTO t VALUES (2, null, 20);").unwrap();
    db.execute("INSERT INTO t VALUES (3, 'x', 5);").unwrap();

    let (_columns, data) = rows(db.execute("SELECT g, SUM(v) FROM t GROUP BY g;").unwrap());
    // NULLs are grouped together (unlike in a WHERE comparison, where NULL is
    // never equal to anything).
    let null_group = data.iter().find(|row| row[0] == Value::Null).unwrap();
    assert_eq!(null_group[1], Value::Int(30));
    assert_eq!(data.len(), 2);
}

#[test]
fn count_distinct_deduplicates() {
    let mut db = Database::new();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, v INT);")
        .unwrap();
    for (i, v) in [10, 10, 20, 20, 20, 30].iter().enumerate() {
        db.execute(&format!("INSERT INTO t VALUES ({}, {v});", i + 1))
            .unwrap();
    }

    let (_columns, data) = rows(
        db.execute("SELECT COUNT(v), COUNT(DISTINCT v), SUM(DISTINCT v) FROM t;")
            .unwrap(),
    );
    assert_eq!(data[0][0], Value::Int(6), "COUNT counts every value");
    assert_eq!(data[0][1], Value::Int(3), "COUNT DISTINCT counts 10,20,30");
    assert_eq!(data[0][2], Value::Int(60), "SUM DISTINCT is 10+20+30");
}

#[test]
fn having_filters_groups_after_aggregation() {
    let mut db = seeded_sales();
    let (_columns, data) = rows(
        db.execute(
            "SELECT region, COUNT(*) FROM sales GROUP BY region HAVING COUNT(*) > 1 \
             ORDER BY region;",
        )
        .unwrap(),
    );
    // Only north (3) and south (2) survive; east (1) is filtered out.
    let regions: Vec<String> = data.iter().map(|row| row[0].to_string()).collect();
    assert_eq!(regions, vec!["north", "south"]);
}

#[test]
fn having_can_reference_an_aggregate_not_in_the_select_list() {
    let mut db = seeded_sales();
    let (_columns, data) = rows(
        db.execute("SELECT region FROM sales GROUP BY region HAVING SUM(amount) >= 600;")
            .unwrap(),
    );
    // Only north sums to >= 600.
    assert_eq!(data, vec![vec![Value::Text("north".into())]]);
}

#[test]
fn having_combines_aggregates_with_and() {
    let mut db = seeded_sales();
    // HAVING follows the SQL-standard model: its leaves are aggregate
    // expressions (or grouping columns), evaluated before output aliases
    // exist — so it references COUNT(*) and SUM(amount) directly, not `n`.
    let (columns, data) = rows(
        db.execute(
            "SELECT region, COUNT(*) AS n FROM sales GROUP BY region \
             HAVING COUNT(*) > 1 AND SUM(amount) > 100 ORDER BY region;",
        )
        .unwrap(),
    );
    assert_eq!(columns, vec!["region", "n"]);
    // north: count=3, sum=600 (passes). south: count=2, sum=50 (fails sum>100).
    assert_eq!(data, vec![vec![Value::Text("north".into()), Value::Int(3)]]);
}

#[test]
fn having_can_reference_a_grouping_column() {
    let mut db = seeded_sales();
    let (_columns, data) = rows(
        db.execute(
            "SELECT region, COUNT(*) FROM sales GROUP BY region \
             HAVING region > 'east' ORDER BY region;",
        )
        .unwrap(),
    );
    // Only 'north' and 'south' sort after 'east'.
    let regions: Vec<String> = data.iter().map(|row| row[0].to_string()).collect();
    assert_eq!(regions, vec!["north", "south"]);
}

#[test]
fn group_by_multiple_columns() {
    let mut db = Database::new();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, a TEXT, b TEXT, v INT);")
        .unwrap();
    db.execute("INSERT INTO t VALUES (1, 'x', 'p', 1);")
        .unwrap();
    db.execute("INSERT INTO t VALUES (2, 'x', 'p', 2);")
        .unwrap();
    db.execute("INSERT INTO t VALUES (3, 'x', 'q', 4);")
        .unwrap();

    let (_columns, data) = rows(
        db.execute("SELECT a, b, SUM(v) FROM t GROUP BY a, b ORDER BY b;")
            .unwrap(),
    );
    assert_eq!(
        data,
        vec![
            vec![
                Value::Text("x".into()),
                Value::Text("p".into()),
                Value::Int(3)
            ],
            vec![
                Value::Text("x".into()),
                Value::Text("q".into()),
                Value::Int(4)
            ],
        ]
    );
}

#[test]
fn aggregate_respects_a_where_clause_first() {
    let mut db = seeded_sales();
    // WHERE filters rows before grouping.
    let (_columns, data) = rows(
        db.execute(
            "SELECT region, COUNT(*) FROM sales WHERE amount >= 100 \
             GROUP BY region ORDER BY region;",
        )
        .unwrap(),
    );
    // amount>=100 keeps north(100,200,300) and nothing else (south 50, NULLs excluded).
    assert_eq!(data, vec![vec![Value::Text("north".into()), Value::Int(3)]]);
}

#[test]
fn min_max_work_on_text() {
    let mut db = Database::new();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    for (i, n) in ["mallory", "alice", "bob"].iter().enumerate() {
        db.execute(&format!("INSERT INTO t VALUES ({}, '{n}');", i + 1))
            .unwrap();
    }
    let (_columns, data) = rows(db.execute("SELECT MIN(name), MAX(name) FROM t;").unwrap());
    assert_eq!(data[0][0], Value::Text("alice".into()));
    assert_eq!(data[0][1], Value::Text("mallory".into()));
}

#[test]
fn sum_of_a_text_column_is_a_type_error() {
    let mut db = Database::new();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    db.execute("INSERT INTO t VALUES (1, 'x');").unwrap();
    assert!(
        db.execute("SELECT SUM(name) FROM t;").is_err(),
        "SUM over a TEXT column must be rejected"
    );
    assert!(
        db.execute("SELECT AVG(name) FROM t;").is_err(),
        "AVG over a TEXT column must be rejected"
    );
}

#[test]
fn selecting_an_ungrouped_column_is_rejected() {
    let mut db = seeded_sales();
    // `amount` is neither a grouping key nor wrapped in an aggregate.
    assert!(
        db.execute("SELECT region, amount FROM sales GROUP BY region;")
            .is_err(),
        "a bare column not in GROUP BY must be rejected"
    );
}

#[test]
fn explain_shows_the_aggregate_step() {
    let mut db = seeded_sales();
    let plan = db
        .execute("EXPLAIN SELECT region, COUNT(*) FROM sales GROUP BY region HAVING COUNT(*) > 1;")
        .unwrap();
    match plan {
        QueryResult::Plan { plan } => {
            assert!(
                plan.contains("HashAggregate"),
                "plan names the aggregate: {plan}"
            );
            assert!(
                plan.contains("group_by=[region]"),
                "plan shows the key: {plan}"
            );
            assert!(
                plan.contains("Having"),
                "plan shows the HAVING step: {plan}"
            );
        }
        other => panic!("expected a plan, got {other:?}"),
    }
}

#[test]
fn bare_count_star_is_unchanged() {
    // The pre-existing fast path must still produce exactly the same shape.
    let mut db = seeded_sales();
    let result = db.execute("SELECT COUNT(*) FROM sales;").unwrap();
    assert_eq!(
        result,
        QueryResult::Rows {
            columns: vec!["count".into()],
            rows: vec![vec![Value::Int(6)]],
        }
    );
}
