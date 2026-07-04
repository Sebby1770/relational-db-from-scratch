use std::env;
use std::io::{self, Write};

use relational_db_from_scratch::Database;

fn main() -> io::Result<()> {
    let mut db = match env::args().nth(1) {
        Some(path) => Database::open(path)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?,
        None => Database::new(),
    };
    let stdin = io::stdin();

    println!("relational-db-from-scratch");
    if let Some(directory) = db.data_directory() {
        println!("Persistent storage: {}", directory.display());
    } else {
        println!("In-memory mode. Pass a data directory to enable WAL + snapshots.");
    }
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

        if trimmed.eq_ignore_ascii_case(".checkpoint") {
            match db.execute("CHECKPOINT;") {
                Ok(result) => println!("{}", result.format_for_display()),
                Err(error) => eprintln!("error: {error}"),
            }
            continue;
        }

        if let Some(output) = handle_meta_command(&mut db, trimmed) {
            println!("{output}");
            continue;
        }

        let started = std::time::Instant::now();
        match db.execute(trimmed) {
            Ok(result) => {
                println!("{}", result.format_for_display());
                let elapsed = started.elapsed();
                if elapsed.as_millis() > 0 {
                    eprintln!("({} ms)", elapsed.as_millis());
                }
            }
            Err(error) => eprintln!("error: {error}"),
        }
    }

    Ok(())
}

fn describe_storage(db: &Database) -> String {
    match db.data_directory() {
        Some(path) => format!(
            "storage directory: {}\nrun .checkpoint to flush database.snapshot and truncate wal.log",
            path.display()
        ),
        None => "in-memory mode (no persistence directory configured)".into(),
    }
}

fn handle_meta_command(db: &mut Database, input: &str) -> Option<String> {
    let mut parts = input.split_whitespace();
    let command = parts.next()?;

    match command.to_ascii_lowercase().as_str() {
        ".help" => Some(
            [
                "Meta commands:",
                "  .tables            list tables",
                "  .schema <table>    show table schema and indexes",
                "  .storage           show persistence directory status",
                "  .checkpoint        flush snapshot + truncate WAL",
                "  .help              show this help",
                "  .quit / .exit      leave the REPL",
                "",
                "Launch with a data directory to enable WAL logging and CHECKPOINT snapshots.",
            ]
            .join("\n"),
        ),
        ".storage" => Some(describe_storage(db)),
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
