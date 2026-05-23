use std::io::{self, Write};

use relational_db_from_scratch::Database;

fn main() -> io::Result<()> {
    let mut db = Database::new();
    let stdin = io::stdin();

    println!("relational-db-from-scratch");
    println!("Type SQL statements or .quit to exit.");

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

        match db.execute(trimmed) {
            Ok(result) => println!("{}", result.format_for_display()),
            Err(error) => eprintln!("error: {error}"),
        }
    }

    Ok(())
}
