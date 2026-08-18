use relational_db_from_scratch::parser::{
    ComparisonOp, InsertSource, JoinType, Predicate, Projection, SelectItem, Statement, parse_sql,
};

#[test]
fn parses_create_table_with_constraints() {
    let statement = parse_sql(
        "CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE NOT NULL, active BOOL);",
    )
    .unwrap();
    assert!(matches!(statement, Statement::CreateTable { .. }));
}

#[test]
fn parses_select_with_order_and_limit() {
    let statement =
        parse_sql("SELECT id, email FROM users WHERE active = true ORDER BY id DESC LIMIT 5;")
            .unwrap();

    match statement {
        Statement::Select(query) => {
            assert!(matches!(query.projection, Projection::Columns(_)));
            assert!(!query.order_by.is_empty());
            assert_eq!(query.limit, Some(5));
        }
        other => panic!("unexpected statement: {other:?}"),
    }
}

#[test]
fn parses_predicate_precedence() {
    let statement = parse_sql("SELECT id FROM users WHERE a = 1 AND b = 2 OR c = 3;").unwrap();
    match statement {
        Statement::Select(query) => match query.predicate {
            Some(Predicate::Or(left, right)) => {
                assert!(matches!(*left, Predicate::And(_, _)));
                assert!(matches!(*right, Predicate::Comparison { .. }));
            }
            other => panic!("unexpected predicate: {other:?}"),
        },
        other => panic!("unexpected statement: {other:?}"),
    }
}

#[test]
fn parses_explain_and_transaction_commands() {
    assert!(matches!(
        parse_sql("EXPLAIN SELECT * FROM users;").unwrap(),
        Statement::Explain(_)
    ));
    assert!(matches!(parse_sql("BEGIN;").unwrap(), Statement::Begin));
    assert!(matches!(parse_sql("COMMIT;").unwrap(), Statement::Commit));
    assert!(matches!(
        parse_sql("ROLLBACK;").unwrap(),
        Statement::Rollback
    ));
}

#[test]
fn rejects_unterminated_string_literals() {
    assert!(parse_sql("SELECT * FROM users WHERE email = 'open;").is_err());
}

#[test]
fn parses_comparison_operators() {
    let statement = parse_sql("SELECT id FROM scores WHERE points >= 20;").unwrap();
    match statement {
        Statement::Select(query) => match query.predicate {
            Some(Predicate::Comparison { op, .. }) => {
                assert_eq!(op, ComparisonOp::Gte);
            }
            other => panic!("unexpected predicate: {other:?}"),
        },
        other => panic!("unexpected statement: {other:?}"),
    }
}

#[test]
fn parses_join_group_like_offset_and_copy() {
    let join = parse_sql(
        "SELECT users.name, orders.total FROM users JOIN orders ON users.id = orders.user_id;",
    )
    .unwrap();
    match join {
        Statement::Select(query) => {
            assert_eq!(query.joins.len(), 1);
            assert_eq!(query.joins[0].join_type, JoinType::Inner);
            assert_eq!(query.joins[0].left, "users.id");
            assert_eq!(query.joins[0].right, "orders.user_id");
            assert_eq!(query.offset, None);
        }
        other => panic!("unexpected statement: {other:?}"),
    }

    let grouped = parse_sql("SELECT dept, COUNT(*), SUM(n) FROM emp GROUP BY dept;").unwrap();
    match grouped {
        Statement::Select(query) => match query.projection {
            Projection::Items(items) => {
                assert!(matches!(items[0], SelectItem::Column(_)));
                assert!(matches!(items[1], SelectItem::CountAll));
                assert!(matches!(items[2], SelectItem::Sum(_)));
                assert_eq!(query.group_by, vec!["dept"]);
            }
            other => panic!("unexpected projection: {other:?}"),
        },
        other => panic!("unexpected statement: {other:?}"),
    }

    let like = parse_sql("SELECT word FROM words WHERE word LIKE 'foo%';").unwrap();
    assert!(matches!(
        like,
        Statement::Select(query) if matches!(query.predicate, Some(Predicate::Like { .. }))
    ));

    let limited = parse_sql("SELECT id FROM users ORDER BY id LIMIT 10 OFFSET 5;").unwrap();
    match limited {
        Statement::Select(query) => {
            assert_eq!(query.limit, Some(10));
            assert_eq!(query.offset, Some(5));
        }
        other => panic!("unexpected statement: {other:?}"),
    }

    assert!(matches!(
        parse_sql("COPY users FROM 'users.csv';").unwrap(),
        Statement::CopyFrom { .. }
    ));

    let left =
        parse_sql("SELECT * FROM users LEFT JOIN orders ON users.id = orders.user_id;").unwrap();
    match left {
        Statement::Select(query) => {
            assert_eq!(query.joins[0].join_type, JoinType::Left);
        }
        other => panic!("unexpected statement: {other:?}"),
    }
}

#[test]
fn parses_having_distinct_insert_select_and_richer_predicates() {
    let having =
        parse_sql("SELECT dept, COUNT(*) FROM emp GROUP BY dept HAVING COUNT(*) > 1;").unwrap();
    match having {
        Statement::Select(query) => {
            assert_eq!(query.group_by, vec!["dept"]);
            assert!(!query.distinct);
            match query.having {
                Some(Predicate::Comparison { expr, op, .. }) => {
                    assert!(matches!(expr, SelectItem::CountAll));
                    assert_eq!(op, ComparisonOp::Gt);
                }
                other => panic!("unexpected having: {other:?}"),
            }
        }
        other => panic!("unexpected statement: {other:?}"),
    }

    let distinct = parse_sql("SELECT DISTINCT dept, n FROM emp;").unwrap();
    match distinct {
        Statement::Select(query) => assert!(query.distinct),
        other => panic!("unexpected statement: {other:?}"),
    }

    let insert_select =
        parse_sql("INSERT INTO dest SELECT id, name FROM src WHERE id IN (1, 2);").unwrap();
    match insert_select {
        Statement::Insert {
            table,
            source: InsertSource::Select(query),
        } => {
            assert_eq!(table, "dest");
            match query.predicate {
                Some(Predicate::InList { values, .. }) => {
                    assert_eq!(values.len(), 2);
                }
                other => panic!("unexpected source select predicate: {other:?}"),
            }
        }
        other => panic!("unexpected statement: {other:?}"),
    }

    let between = parse_sql("SELECT id FROM scores WHERE points BETWEEN 10 AND 20;").unwrap();
    assert!(matches!(
        between,
        Statement::Select(query) if matches!(query.predicate, Some(Predicate::Between { .. }))
    ));

    let aggs = parse_sql("SELECT MIN(n), MAX(n), AVG(n) FROM emp;").unwrap();
    match aggs {
        Statement::Select(query) => match query.projection {
            Projection::Items(items) => {
                assert!(matches!(items[0], SelectItem::Min(_)));
                assert!(matches!(items[1], SelectItem::Max(_)));
                assert!(matches!(items[2], SelectItem::Avg(_)));
            }
            other => panic!("unexpected projection: {other:?}"),
        },
        other => panic!("unexpected statement: {other:?}"),
    }
}
