use std::cmp::Ordering;

use crate::db::Database;
use crate::error::{DbError, Result};
use crate::optimizer::TableStats;
use crate::parser::{
    Assignment, ComparisonOp, OrderBy, Predicate, Projection, SortDirection, Statement,
};
use crate::row::Row;
use crate::schema::{TableSchema, normalize_identifier};
use crate::storage::{RowId, Table};
use crate::transaction::UndoRecord;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryResult {
    TableCreated {
        table: String,
    },
    IndexCreated {
        index: String,
    },
    TableDropped {
        table: String,
    },
    IndexDropped {
        index: String,
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
    Plan {
        plan: String,
    },
    TransactionStarted {
        id: u64,
    },
    TransactionCommitted {
        id: u64,
    },
    TransactionRolledBack {
        id: u64,
    },
    Analyzed {
        tables: Vec<String>,
    },
    Checkpoint {
        message: String,
    },
}

impl QueryResult {
    pub fn format_for_display(&self) -> String {
        match self {
            QueryResult::TableCreated { table } => format!("created table {table}"),
            QueryResult::IndexCreated { index } => format!("created index {index}"),
            QueryResult::TableDropped { table } => format!("dropped table {table}"),
            QueryResult::IndexDropped { index } => format!("dropped index {index}"),
            QueryResult::RowsInserted { count } => format!("inserted {count} row(s)"),
            QueryResult::RowsUpdated { count } => format!("updated {count} row(s)"),
            QueryResult::RowsDeleted { count } => format!("deleted {count} row(s)"),
            QueryResult::Rows { columns, rows } => format_rows(columns, rows),
            QueryResult::Plan { plan } => plan.clone(),
            QueryResult::TransactionStarted { id } => format!("transaction {id} started"),
            QueryResult::TransactionCommitted { id } => format!("transaction {id} committed"),
            QueryResult::TransactionRolledBack { id } => {
                format!("transaction {id} rolled back")
            }
            QueryResult::Analyzed { tables } => format!("analyzed {}", tables.join(", ")),
            QueryResult::Checkpoint { message } => message.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BoundPredicate {
    Comparison {
        column_index: usize,
        column_name: String,
        op: ComparisonOp,
        value: Value,
    },
    And(Box<BoundPredicate>, Box<BoundPredicate>),
    Or(Box<BoundPredicate>, Box<BoundPredicate>),
}

pub(crate) fn execute_statement(db: &mut Database, statement: Statement) -> Result<QueryResult> {
    match statement {
        Statement::CreateTable { name, columns } => {
            let schema = TableSchema::new(name.clone(), columns)?;
            let table_name = normalize_identifier(&name);
            db.acquire_write_lock(&table_name)?;
            db.create_table(schema)?;
            db.record_undo(UndoRecord::CreateTable {
                table: table_name.clone(),
            });
            Ok(QueryResult::TableCreated { table: table_name })
        }
        Statement::DropTable { name } => {
            let table_name = normalize_identifier(&name);
            db.acquire_write_lock(&table_name)?;
            db.drop_table(&table_name)?;
            Ok(QueryResult::TableDropped { table: table_name })
        }
        Statement::DropIndex { name } => {
            let index_name = normalize_identifier(&name);
            let table_name = db.table_for_index(&index_name)?;
            db.acquire_write_lock(&table_name)?;
            let (_, index_name) = db.drop_index(&index_name)?;
            Ok(QueryResult::IndexDropped { index: index_name })
        }
        Statement::CreateIndex {
            name,
            table,
            column,
            unique,
        } => {
            let index_name = normalize_identifier(&name);
            let table_name = normalize_identifier(&table);
            db.acquire_write_lock(&table_name)?;
            db.create_index(index_name.clone(), table_name.clone(), column, unique)?;
            db.record_undo(UndoRecord::CreateIndex {
                table: table_name,
                index: index_name.clone(),
            });
            Ok(QueryResult::IndexCreated { index: index_name })
        }
        Statement::Insert { table, values } => insert(db, &table, values),
        Statement::Select {
            table,
            projection,
            predicate,
            order_by,
            limit,
        } => select(
            db,
            &table,
            &projection,
            predicate.as_ref(),
            order_by.as_ref(),
            limit,
        ),
        Statement::Update {
            table,
            assignments,
            predicate,
        } => update(db, &table, &assignments, predicate.as_ref()),
        Statement::Delete { table, predicate } => delete(db, &table, predicate.as_ref()),
        Statement::Explain(statement) => explain(db, &statement),
        Statement::Begin => {
            let id = db.begin()?.0;
            Ok(QueryResult::TransactionStarted { id })
        }
        Statement::Commit => {
            let id = db.commit()?.0;
            Ok(QueryResult::TransactionCommitted { id })
        }
        Statement::Rollback => {
            let id = db.rollback()?.0;
            Ok(QueryResult::TransactionRolledBack { id })
        }
        Statement::Analyze { table } => analyze(db, table.as_deref()),
        Statement::Checkpoint => {
            let message = db.checkpoint()?;
            Ok(QueryResult::Checkpoint { message })
        }
    }
}

fn insert(db: &mut Database, table_name: &str, row: Row) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_write_lock(&table_name)?;
    let row_id = {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        table.insert(row)?
    };

    db.record_undo(UndoRecord::Insert {
        table: table_name,
        row_id,
    });
    Ok(QueryResult::RowsInserted { count: 1 })
}

fn select(
    db: &mut Database,
    table_name: &str,
    projection: &Projection,
    predicate: Option<&Predicate>,
    order_by: Option<&OrderBy>,
    limit: Option<usize>,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_read_lock(&table_name)?;
    let table = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    let predicate = resolve_predicate(&table.schema, predicate)?;
    let mut row_ids = candidate_row_ids(table, predicate.as_ref());

    row_ids.retain(|row_id| {
        table
            .row(*row_id)
            .is_some_and(|row| matches_predicate(row, predicate.as_ref()))
    });

    if let Some(order_by) = order_by {
        let order_column = resolve_column(&table.schema, &order_by.column)?;
        row_ids.sort_by(|left_id, right_id| {
            let left = &table.row(*left_id).expect("candidate row exists")[order_column];
            let right = &table.row(*right_id).expect("candidate row exists")[order_column];
            let ordering = left.compare_same_type(right).unwrap_or(Ordering::Equal);

            match order_by.direction {
                SortDirection::Asc => ordering,
                SortDirection::Desc => ordering.reverse(),
            }
        });
    }

    if let Some(limit) = limit {
        row_ids.truncate(limit);
    }

    if projection == &Projection::CountAll {
        return Ok(QueryResult::Rows {
            columns: vec!["count".into()],
            rows: vec![vec![Value::Int(row_ids.len() as i64)]],
        });
    }

    let (columns, column_indexes) = resolve_projection(&table.schema, projection)?;
    let rows = row_ids
        .into_iter()
        .filter_map(|row_id| table.row(row_id))
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
    db.acquire_write_lock(&table_name)?;
    let changes = {
        let table = db
            .tables
            .get(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        let predicate = resolve_predicate(&table.schema, predicate)?;
        let assignments = resolve_assignments(&table.schema, assignments)?;
        let mut row_ids = candidate_row_ids(table, predicate.as_ref());

        row_ids.retain(|row_id| {
            table
                .row(*row_id)
                .is_some_and(|row| matches_predicate(row, predicate.as_ref()))
        });

        row_ids
            .into_iter()
            .map(|row_id| {
                let mut new_row = table.row(row_id).expect("candidate row exists").clone();
                for (index, value) in &assignments {
                    new_row[*index] = value.clone();
                }
                Ok((row_id, new_row))
            })
            .collect::<Result<Vec<_>>>()?
    };

    let mut undo_records = Vec::new();
    {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;

        for (row_id, new_row) in changes {
            let old_row = table.replace_row(row_id, new_row)?;
            undo_records.push(UndoRecord::Update {
                table: table_name.clone(),
                row_id,
                old_row,
            });
        }
    }

    let updated = undo_records.len();
    for undo in undo_records {
        db.record_undo(undo);
    }

    Ok(QueryResult::RowsUpdated { count: updated })
}

fn delete(
    db: &mut Database,
    table_name: &str,
    predicate: Option<&Predicate>,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_write_lock(&table_name)?;
    let row_ids = {
        let table = db
            .tables
            .get(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        let predicate = resolve_predicate(&table.schema, predicate)?;
        let mut row_ids = candidate_row_ids(table, predicate.as_ref());

        row_ids.retain(|row_id| {
            table
                .row(*row_id)
                .is_some_and(|row| matches_predicate(row, predicate.as_ref()))
        });
        row_ids
    };

    let mut undo_records = Vec::new();
    {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;

        for row_id in row_ids {
            let stored = table.delete_row(row_id)?;
            undo_records.push(UndoRecord::Delete {
                table: table_name.clone(),
                stored,
            });
        }
    }

    let deleted = undo_records.len();
    for undo in undo_records {
        db.record_undo(undo);
    }

    Ok(QueryResult::RowsDeleted { count: deleted })
}

fn analyze(db: &mut Database, table: Option<&str>) -> Result<QueryResult> {
    let table_names = if let Some(table) = table {
        let table = normalize_identifier(table);
        if !db.tables.contains_key(&table) {
            return Err(DbError::TableNotFound(table));
        }
        vec![table]
    } else {
        let mut tables = db.tables.keys().cloned().collect::<Vec<_>>();
        tables.sort();
        tables
    };

    let mut summaries = Vec::new();
    for table_name in table_names {
        let table = db
            .tables
            .get(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        let stats = TableStats::from_rows(&table.schema, table.rows().cloned());
        let summary = format!(
            "{} ({} rows, {} indexed columns)",
            table_name,
            stats.row_count,
            stats.distinct_values.len()
        );
        db.stats.insert(table_name, stats);
        summaries.push(summary);
    }

    Ok(QueryResult::Analyzed { tables: summaries })
}

fn explain(db: &Database, statement: &Statement) -> Result<QueryResult> {
    let plan = match statement {
        Statement::Select {
            table,
            projection,
            predicate,
            order_by,
            limit,
        } => explain_select(
            db,
            table,
            projection,
            predicate.as_ref(),
            order_by.as_ref(),
            *limit,
        )?,
        Statement::Insert { table, .. } => {
            format!("Insert\n  Table: {}", normalize_identifier(table))
        }
        Statement::Update {
            table, predicate, ..
        } => {
            let table_name = normalize_identifier(table);
            let table_ref = db
                .tables
                .get(&table_name)
                .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
            let predicate = resolve_predicate(&table_ref.schema, predicate.as_ref())?;
            format!(
                "Update\n  Table: {table_name}\n  Access: {}",
                describe_access_path(table_ref, predicate.as_ref(), db.stats.get(&table_name),)
            )
        }
        Statement::Delete { table, predicate } => {
            let table_name = normalize_identifier(table);
            let table_ref = db
                .tables
                .get(&table_name)
                .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
            let predicate = resolve_predicate(&table_ref.schema, predicate.as_ref())?;
            format!(
                "Delete\n  Table: {table_name}\n  Access: {}",
                describe_access_path(table_ref, predicate.as_ref(), db.stats.get(&table_name),)
            )
        }
        other => format!("{other:?}"),
    };

    Ok(QueryResult::Plan { plan })
}

fn explain_select(
    db: &Database,
    table: &str,
    projection: &Projection,
    predicate: Option<&Predicate>,
    order_by: Option<&OrderBy>,
    limit: Option<usize>,
) -> Result<String> {
    let table_name = normalize_identifier(table);
    let table = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    let predicate = resolve_predicate(&table.schema, predicate)?;
    let projection = match projection {
        Projection::All => "*".into(),
        Projection::Columns(columns) => columns.join(", "),
        Projection::CountAll => "COUNT(*)".into(),
    };
    let mut lines = vec![
        "Select".to_string(),
        format!("  Table: {table_name}"),
        format!("  Projection: {projection}"),
        format!(
            "  Access: {}",
            describe_access_path(table, predicate.as_ref(), db.stats.get(&table_name))
        ),
    ];

    if let Some(order_by) = order_by {
        lines.push(format!(
            "  Sort: {} {:?}",
            normalize_identifier(&order_by.column),
            order_by.direction
        ));
    }

    if let Some(limit) = limit {
        lines.push(format!("  Limit: {limit}"));
    }

    Ok(lines.join("\n"))
}

fn describe_access_path(
    table: &Table,
    predicate: Option<&BoundPredicate>,
    stats: Option<&TableStats>,
) -> String {
    if let Some((column_index, value)) = index_probe(predicate)
        && let Some(index) = table.index_on_column(column_index)
    {
        let mut plan = format!(
            "IndexLookup(index={}, column={}, key={})",
            index.name, index.column, value
        );
        if let Some(stats) = stats {
            let estimated = stats.estimate_equality_rows(&index.column);
            plan.push_str(&format!(", est_rows={estimated}"));
        }
        return plan;
    }

    if let Some(stats) = stats {
        return format!("SeqScan(rows={})", stats.row_count);
    }

    "SeqScan".into()
}

fn candidate_row_ids(table: &Table, predicate: Option<&BoundPredicate>) -> Vec<RowId> {
    if let Some((column_index, value)) = index_probe(predicate)
        && let Some(index) = table.index_on_column(column_index)
    {
        return index.probe(&value);
    }

    table.row_ids()
}

fn index_probe(predicate: Option<&BoundPredicate>) -> Option<(usize, Value)> {
    match predicate {
        Some(BoundPredicate::Comparison {
            column_index,
            op: ComparisonOp::Eq,
            value,
            ..
        }) => Some((*column_index, value.clone())),
        Some(BoundPredicate::And(left, right)) => {
            index_probe(Some(left)).or_else(|| index_probe(Some(right)))
        }
        _ => None,
    }
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
                let index = resolve_column(schema, &column)?;
                names.push(column);
                indexes.push(index);
            }

            Ok((names, indexes))
        }
        Projection::CountAll => Ok((vec!["count".into()], Vec::new())),
    }
}

fn resolve_assignments(
    schema: &TableSchema,
    assignments: &[Assignment],
) -> Result<Vec<(usize, Value)>> {
    let mut resolved = Vec::new();

    for assignment in assignments {
        let column = normalize_identifier(&assignment.column);
        let index = resolve_column(schema, &column)?;
        schema.validate_value(index, &assignment.value)?;
        resolved.push((index, assignment.value.clone()));
    }

    Ok(resolved)
}

fn resolve_predicate(
    schema: &TableSchema,
    predicate: Option<&Predicate>,
) -> Result<Option<BoundPredicate>> {
    let Some(predicate) = predicate else {
        return Ok(None);
    };

    resolve_predicate_inner(schema, predicate).map(Some)
}

fn resolve_predicate_inner(schema: &TableSchema, predicate: &Predicate) -> Result<BoundPredicate> {
    match predicate {
        Predicate::Comparison { column, op, value } => {
            let column = normalize_identifier(column);
            let index = resolve_column(schema, &column)?;
            schema.validate_value(index, value)?;
            Ok(BoundPredicate::Comparison {
                column_index: index,
                column_name: column,
                op: *op,
                value: value.clone(),
            })
        }
        Predicate::And(left, right) => Ok(BoundPredicate::And(
            Box::new(resolve_predicate_inner(schema, left)?),
            Box::new(resolve_predicate_inner(schema, right)?),
        )),
        Predicate::Or(left, right) => Ok(BoundPredicate::Or(
            Box::new(resolve_predicate_inner(schema, left)?),
            Box::new(resolve_predicate_inner(schema, right)?),
        )),
    }
}

fn resolve_column(schema: &TableSchema, column: &str) -> Result<usize> {
    let column = normalize_identifier(column);
    schema
        .column_index(&column)
        .ok_or(DbError::ColumnNotFound(column))
}

fn matches_predicate(row: &Row, predicate: Option<&BoundPredicate>) -> bool {
    match predicate {
        Some(BoundPredicate::Comparison {
            column_index,
            op,
            value,
            ..
        }) => compare_values(&row[*column_index], *op, value),
        Some(BoundPredicate::And(left, right)) => {
            matches_predicate(row, Some(left)) && matches_predicate(row, Some(right))
        }
        Some(BoundPredicate::Or(left, right)) => {
            matches_predicate(row, Some(left)) || matches_predicate(row, Some(right))
        }
        None => true,
    }
}

fn compare_values(left: &Value, op: ComparisonOp, right: &Value) -> bool {
    if left.is_null() || right.is_null() {
        return false;
    }

    match op {
        ComparisonOp::Eq => left == right,
        ComparisonOp::Ne => left != right,
        ComparisonOp::Lt => left
            .compare_same_type(right)
            .is_some_and(|ordering| ordering == Ordering::Less),
        ComparisonOp::Lte => left
            .compare_same_type(right)
            .is_some_and(|ordering| matches!(ordering, Ordering::Less | Ordering::Equal)),
        ComparisonOp::Gt => left
            .compare_same_type(right)
            .is_some_and(|ordering| ordering == Ordering::Greater),
        ComparisonOp::Gte => left
            .compare_same_type(right)
            .is_some_and(|ordering| matches!(ordering, Ordering::Greater | Ordering::Equal)),
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
