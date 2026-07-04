use relational_db_from_scratch::{Database, QueryResult};

#[test]
fn drop_table_and_index_work() {
    let mut db = Database::new();
    db.execute("CREATE TABLE users (id INT PRIMARY KEY, email TEXT);")
        .unwrap();
    db.execute("INSERT INTO users VALUES (1, 'ada@example.com');")
        .unwrap();
    db.execute("CREATE INDEX users_email_idx ON users(email);")
        .unwrap();

    assert!(matches!(
        db.execute("DROP INDEX users_email_idx;").unwrap(),
        QueryResult::IndexDropped { .. }
    ));
    assert!(matches!(
        db.execute("DROP TABLE users;").unwrap(),
        QueryResult::TableDropped { .. }
    ));
    assert!(db.execute("SELECT * FROM users;").is_err());
}

#[test]
fn parser_accepts_drop_statements() {
    use relational_db_from_scratch::parser::{Statement, parse_sql};

    assert!(matches!(
        parse_sql("DROP TABLE users;").unwrap(),
        Statement::DropTable { .. }
    ));
    assert!(matches!(
        parse_sql("DROP INDEX users_email_idx;").unwrap(),
        Statement::DropIndex { .. }
    ));
}