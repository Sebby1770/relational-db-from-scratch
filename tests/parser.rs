use relational_db_from_scratch::parser::{
    ComparisonOp, Predicate, Projection, Statement, parse_sql,
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
            assert!(order_by.is_some());
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
