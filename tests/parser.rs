use relational_db_from_scratch::parser::{
    ComparisonOp, JoinType, Predicate, Projection, SelectItem, Statement, parse_sql,
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
        Statement::Select {
            projection,
            order_by,
            limit,
            ..
        } => {
            assert!(matches!(projection, Projection::Columns(_)));
            assert!(!order_by.is_empty());
            assert_eq!(limit, Some(5));
        }
        other => panic!("unexpected statement: {other:?}"),
    }
}

#[test]
fn parses_predicate_precedence() {
    let statement = parse_sql("SELECT id FROM users WHERE a = 1 AND b = 2 OR c = 3;").unwrap();
    match statement {
        Statement::Select {
            predicate: Some(pred),
            ..
        } => match pred {
            Predicate::Or(left, right) => {
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
        Statement::Select {
            predicate: Some(Predicate::Comparison { op, .. }),
            ..
        } => {
            assert_eq!(op, ComparisonOp::Gte);
        }
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
        Statement::Select { joins, offset, .. } => {
            assert_eq!(joins.len(), 1);
            assert_eq!(joins[0].join_type, JoinType::Inner);
            assert_eq!(joins[0].left, "users.id");
            assert_eq!(joins[0].right, "orders.user_id");
            assert_eq!(offset, None);
        }
        other => panic!("unexpected statement: {other:?}"),
    }

    let grouped = parse_sql("SELECT dept, COUNT(*), SUM(n) FROM emp GROUP BY dept;").unwrap();
    match grouped {
        Statement::Select {
            projection: Projection::Items(items),
            group_by,
            ..
        } => {
            assert!(matches!(items[0], SelectItem::Column(_)));
            assert!(matches!(items[1], SelectItem::CountAll));
            assert!(matches!(items[2], SelectItem::Sum(_)));
            assert_eq!(group_by, vec!["dept"]);
        }
        other => panic!("unexpected statement: {other:?}"),
    }

    let like = parse_sql("SELECT word FROM words WHERE word LIKE 'foo%';").unwrap();
    assert!(matches!(
        like,
        Statement::Select {
            predicate: Some(Predicate::Like { .. }),
            ..
        }
    ));

    let limited = parse_sql("SELECT id FROM users ORDER BY id LIMIT 10 OFFSET 5;").unwrap();
    match limited {
        Statement::Select { limit, offset, .. } => {
            assert_eq!(limit, Some(10));
            assert_eq!(offset, Some(5));
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
        Statement::Select { joins, .. } => {
            assert_eq!(joins[0].join_type, JoinType::Left);
        }
        other => panic!("unexpected statement: {other:?}"),
    }
}
