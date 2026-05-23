use crate::db::Database;
use crate::error::{DbError, Result};
use crate::parser::{Assignment, Predicate, Projection, Statement};
use crate::row::Row;
use crate::schema::{TableSchema, normalize_identifier};
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryResult {
    TableCreated {
        table: String,
    },
    RowsInserted {
        count: usize,
    },
    RowsUpdated {
        count: usize,
    },
    RowsDeleted {
        count: usize,
    },
    Rows {
        columns: Vec<String>,
        rows: Vec<Row>,
    },
}

impl QueryResult {
    pub fn format_for_display(&self) -> String {
        match self {
            QueryResult::TableCreated { table } => format!("created table {table}"),
            QueryResult::RowsInserted { count } => format!("inserted {count} row(s)"),
            QueryResult::RowsUpdated { count } => format!("updated {count} row(s)"),
            QueryResult::RowsDeleted { count } => format!("deleted {count} row(s)"),
            QueryResult::Rows { columns, rows } => format_rows(columns, rows),
        }
    }
}

pub(crate) fn execute_statement(db: &mut Database, statement: Statement) -> Result<QueryResult> {
    match statement {
        Statement::CreateTable { name, columns } => {
            let schema = TableSchema::new(name.clone(), columns)?;
            db.create_table(schema)?;
            Ok(QueryResult::TableCreated {
                table: normalize_identifier(&name),
            })
        }
        Statement::Insert { table, values } => {
            db.insert(&table, values)?;
            Ok(QueryResult::RowsInserted { count: 1 })
        }
        Statement::Select {
            table,
            projection,
            predicate,
        } => select(db, &table, &projection, predicate.as_ref()),
        Statement::Update {
            table,
            assignments,
            predicate,
        } => update(db, &table, &assignments, predicate.as_ref()),
        Statement::Delete { table, predicate } => delete(db, &table, predicate.as_ref()),
    }
}

fn select(
    db: &Database,
    table_name: &str,
    projection: &Projection,
    predicate: Option<&Predicate>,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    let table = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    let predicate = resolve_predicate(&table.schema, predicate)?;
    let (columns, column_indexes) = resolve_projection(&table.schema, projection)?;

    let rows = table
        .rows()
        .iter()
        .filter(|row| matches_predicate(row, predicate.as_ref()))
        .map(|row| {
            column_indexes
                .iter()
                .map(|index| row[*index].clone())
                .collect::<Row>()
        })
        .collect();

    Ok(QueryResult::Rows { columns, rows })
}

fn update(
    db: &mut Database,
    table_name: &str,
    assignments: &[Assignment],
    predicate: Option<&Predicate>,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    let table = db
        .tables
        .get_mut(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    let predicate = resolve_predicate(&table.schema, predicate)?;
    let assignments = resolve_assignments(&table.schema, assignments)?;
    let mut updated = 0;

    for row in table.rows_mut() {
        if matches_predicate(row, predicate.as_ref()) {
            for (index, value) in &assignments {
                row[*index] = value.clone();
            }
            updated += 1;
        }
    }

    Ok(QueryResult::RowsUpdated { count: updated })
}

fn delete(
    db: &mut Database,
    table_name: &str,
    predicate: Option<&Predicate>,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    let table = db
        .tables
        .get_mut(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    let predicate = resolve_predicate(&table.schema, predicate)?;
    let deleted = table.delete_where(|row| matches_predicate(row, predicate.as_ref()));

    Ok(QueryResult::RowsDeleted { count: deleted })
}

fn resolve_projection(
    schema: &TableSchema,
    projection: &Projection,
) -> Result<(Vec<String>, Vec<usize>)> {
    match projection {
        Projection::All => Ok((
            schema.column_names(),
            (0..schema.columns.len()).collect::<Vec<_>>(),
        )),
        Projection::Columns(columns) => {
            let mut names = Vec::new();
            let mut indexes = Vec::new();

            for column in columns {
                let column = normalize_identifier(column);
                let index = schema
                    .column_index(&column)
                    .ok_or_else(|| DbError::ColumnNotFound(column.clone()))?;
                names.push(column);
                indexes.push(index);
            }

            Ok((names, indexes))
        }
    }
}

fn resolve_assignments(
    schema: &TableSchema,
    assignments: &[Assignment],
) -> Result<Vec<(usize, Value)>> {
    let mut resolved = Vec::new();

    for assignment in assignments {
        let column = normalize_identifier(&assignment.column);
        let index = schema
            .column_index(&column)
            .ok_or_else(|| DbError::ColumnNotFound(column.clone()))?;
        schema.validate_value(index, &assignment.value)?;
        resolved.push((index, assignment.value.clone()));
    }

    Ok(resolved)
}

fn resolve_predicate(
    schema: &TableSchema,
    predicate: Option<&Predicate>,
) -> Result<Option<(usize, Value)>> {
    let Some(predicate) = predicate else {
        return Ok(None);
    };

    let column = normalize_identifier(&predicate.column);
    let index = schema
        .column_index(&column)
        .ok_or_else(|| DbError::ColumnNotFound(column.clone()))?;
    schema.validate_value(index, &predicate.value)?;

    Ok(Some((index, predicate.value.clone())))
}

fn matches_predicate(row: &Row, predicate: Option<&(usize, Value)>) -> bool {
    match predicate {
        Some((index, value)) => &row[*index] == value,
        None => true,
    }
}

fn format_rows(columns: &[String], rows: &[Row]) -> String {
    if columns.is_empty() {
        return "0 column(s)".into();
    }

    let mut widths = columns
        .iter()
        .map(|column| column.len())
        .collect::<Vec<_>>();
    let rendered_rows = rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(index, value)| {
                    let rendered = value.to_string();
                    widths[index] = widths[index].max(rendered.len());
                    rendered
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let header = columns
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column:<width$}", width = widths[index]))
        .collect::<Vec<_>>()
        .join(" | ");
    let separator = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("-+-");
    let body = rendered_rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(index, value)| format!("{value:<width$}", width = widths[index]))
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n");

    if body.is_empty() {
        format!("{header}\n{separator}\n(0 rows)")
    } else {
        format!("{header}\n{separator}\n{body}")
    }
}
