use std::io::{self, Write};

use relational_db_from_scratch::Database;

fn main() -> io::Result<()> {
    let mut db = Database::new();
    let stdin = io::stdin();

    println!("relational-db-from-scratch");
    println!("Type SQL statements or .help for meta commands.");

    loop {
        print!("db> ");
        io::stdout().flush()?;

        let mut input = String::new();
        let bytes = stdin.read_line(&mut input)?;

        if bytes == 0 {
            break;
        }

        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case(".quit") || trimmed.eq_ignore_ascii_case(".exit") {
            break;
        }

        if trimmed.is_empty() {
            continue;
        }

        if let Some(output) = handle_meta_command(&db, trimmed) {
            println!("{output}");
            continue;
        }

        match db.execute(trimmed) {
            Ok(result) => println!("{}", result.format_for_display()),
            Err(error) => eprintln!("error: {error}"),
        }
    }

    Ok(())
}

fn handle_meta_command(db: &Database, input: &str) -> Option<String> {
    let mut parts = input.split_whitespace();
    let command = parts.next()?;

    match command.to_ascii_lowercase().as_str() {
        ".help" => Some(
            [
                "Meta commands:",
                "  .tables            list tables",
                "  .schema <table>    show table schema and indexes",
                "  .help              show this help",
                "  .quit / .exit      leave the REPL",
            ]
            .join("\n"),
        ),
        ".tables" => {
            let tables = db.table_names();
            if tables.is_empty() {
                Some("(no tables)".into())
            } else {
                Some(tables.join("\n"))
            }
        }
        ".schema" => {
            let table = parts.next()?;
            match db.describe_table(table) {
                Ok(description) => Some(description),
                Err(error) => Some(format!("error: {error}")),
            }
        }
        _ => None,
    }
}
