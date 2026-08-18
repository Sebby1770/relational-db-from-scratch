use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;

use crate::db::Database;
use crate::error::{DbError, Result};
use crate::optimizer::TableStats;
use crate::parser::{
    AlterAction, Assignment, ComparisonOp, InsertSource, JoinClause, JoinType, OrderBy, Predicate,
    Projection, SelectItem, SelectStatement, SetOp, SortDirection, Statement,
};
use crate::row::Row;
use crate::schema::{Column, DataType, TableSchema, normalize_identifier};
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
    ColumnAdded {
        table: String,
        column: String,
    },
    TableRenamed {
        from: String,
        to: String,
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
    RowsExported {
        path: String,
        count: usize,
    },
}

impl QueryResult {
    pub fn format_for_display(&self) -> String {
        match self {
            QueryResult::TableCreated { table } => format!("created table {table}"),
            QueryResult::IndexCreated { index } => format!("created index {index}"),
            QueryResult::TableDropped { table } => format!("dropped table {table}"),
            QueryResult::IndexDropped { index } => format!("dropped index {index}"),
            QueryResult::ColumnAdded { table, column } => {
                format!("added column {column} to {table}")
            }
            QueryResult::TableRenamed { from, to } => format!("renamed table {from} to {to}"),
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
            QueryResult::RowsExported { path, count } => {
                format!("exported {count} row(s) to {path}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BoundPredicate {
    Comparison {
        expr: BoundSelectItem,
        op: ComparisonOp,
        value: Value,
    },
    Between {
        expr: BoundSelectItem,
        low: Value,
        high: Value,
        negated: bool,
    },
    InList {
        expr: BoundSelectItem,
        values: Vec<Value>,
        negated: bool,
    },
    IsNull {
        expr: BoundSelectItem,
        negated: bool,
    },
    Like {
        column_index: usize,
        pattern: String,
        escape: Option<char>,
    },
    And(Box<BoundPredicate>, Box<BoundPredicate>),
    Or(Box<BoundPredicate>, Box<BoundPredicate>),
}

#[derive(Debug, Clone)]
struct QueryColumn {
    qualifiers: Vec<String>,
    name: String,
    output_name: String,
    data_type: DataType,
}

#[derive(Debug, Clone)]
struct QuerySchema {
    columns: Vec<QueryColumn>,
}

impl QuerySchema {
    fn from_table(table_name: &str, alias: Option<&str>, schema: &TableSchema) -> Self {
        let table_name = normalize_identifier(table_name);
        let mut qualifiers = vec![table_name];
        if let Some(alias) = alias {
            let alias = normalize_identifier(alias);
            if !qualifiers.contains(&alias) {
                qualifiers.push(alias);
            }
        }

        let columns = schema
            .columns
            .iter()
            .map(|column| QueryColumn {
                qualifiers: qualifiers.clone(),
                name: column.name.clone(),
                output_name: column.name.clone(),
                data_type: column.data_type.clone(),
            })
            .collect();

        Self { columns }
    }

    fn merge(mut self, other: QuerySchema) -> Self {
        self.columns.extend(other.columns);
        let mut counts = HashMap::new();
        for column in &self.columns {
            *counts.entry(column.name.clone()).or_insert(0usize) += 1;
        }

        for column in &mut self.columns {
            column.output_name = if counts[&column.name] > 1 {
                format!("{}.{}", display_qualifier(column), column.name)
            } else {
                column.name.clone()
            };
        }

        self
    }

    fn resolve(&self, column: &str) -> Result<usize> {
        let column = normalize_identifier(column);
        if let Some((qualifier, name)) = split_qualified(&column) {
            let matches = matching_indexes(self, |item| {
                item.name == name && item.qualifiers.iter().any(|value| value == &qualifier)
            });
            return unique_match(matches, &column);
        }

        let matches = matching_indexes(self, |item| item.name == column);
        unique_match(matches, &column)
    }

    fn from_output_names(names: &[String]) -> Self {
        let columns = names
            .iter()
            .map(|name| {
                let (qualifiers, bare) = if let Some((qualifier, rest)) = split_qualified(name) {
                    (vec![qualifier], rest)
                } else {
                    (Vec::new(), name.clone())
                };
                QueryColumn {
                    qualifiers,
                    name: bare,
                    output_name: name.clone(),
                    data_type: DataType::Int,
                }
            })
            .collect();
        Self { columns }
    }
}

fn display_qualifier(column: &QueryColumn) -> &str {
    column.qualifiers.last().map(String::as_str).unwrap_or("")
}

fn matching_indexes(schema: &QuerySchema, predicate: impl Fn(&QueryColumn) -> bool) -> Vec<usize> {
    schema
        .columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| predicate(column).then_some(index))
        .collect()
}

fn unique_match(matches: Vec<usize>, column: &str) -> Result<usize> {
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err(DbError::ColumnNotFound(column.to_string())),
        _ => Err(DbError::InvalidStatement(format!(
            "ambiguous column: {column}"
        ))),
    }
}

fn split_qualified(column: &str) -> Option<(String, String)> {
    column
        .split_once('.')
        .map(|(qualifier, name)| (qualifier.to_string(), name.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BoundSelectItem {
    Column {
        index: usize,
        output_name: String,
    },
    CountAll,
    Count {
        index: usize,
        output_name: String,
    },
    Sum {
        index: usize,
        output_name: String,
    },
    Min {
        index: usize,
        output_name: String,
    },
    Max {
        index: usize,
        output_name: String,
    },
    Avg {
        index: usize,
        output_name: String,
    },
    Case {
        when: Box<BoundPredicate>,
        then_value: Value,
        else_value: Value,
        output_name: String,
    },
    Coalesce {
        index: usize,
        fallback: Value,
        output_name: String,
    },
}

impl BoundSelectItem {
    fn output_name(&self) -> String {
        match self {
            BoundSelectItem::Column { output_name, .. }
            | BoundSelectItem::Count { output_name, .. }
            | BoundSelectItem::Sum { output_name, .. }
            | BoundSelectItem::Min { output_name, .. }
            | BoundSelectItem::Max { output_name, .. }
            | BoundSelectItem::Avg { output_name, .. }
            | BoundSelectItem::Case { output_name, .. }
            | BoundSelectItem::Coalesce { output_name, .. } => output_name.clone(),
            BoundSelectItem::CountAll => "count".into(),
        }
    }

    fn is_aggregate(&self) -> bool {
        matches!(
            self,
            BoundSelectItem::CountAll
                | BoundSelectItem::Count { .. }
                | BoundSelectItem::Sum { .. }
                | BoundSelectItem::Min { .. }
                | BoundSelectItem::Max { .. }
                | BoundSelectItem::Avg { .. }
        )
    }
}

struct SelectQuery<'a> {
    table: &'a str,
    alias: Option<&'a str>,
    joins: &'a [JoinClause],
    projection: &'a Projection,
    predicate: Option<&'a Predicate>,
    group_by: &'a [String],
    having: Option<&'a Predicate>,
    distinct: bool,
    order_by: &'a [OrderBy],
    limit: Option<usize>,
    offset: Option<usize>,
}

pub(crate) fn execute_statement(db: &mut Database, statement: Statement) -> Result<QueryResult> {
    match statement {
        Statement::CreateTable {
            name,
            columns,
            if_not_exists,
        } => {
            let schema = TableSchema::new(name.clone(), columns)?;
            let table_name = normalize_identifier(&name);
            db.acquire_write_lock(&table_name)?;
            if if_not_exists && db.tables.contains_key(&table_name) {
                return Ok(QueryResult::TableCreated { table: table_name });
            }
            db.create_table(schema)?;
            db.record_undo(UndoRecord::CreateTable {
                table: table_name.clone(),
            });
            Ok(QueryResult::TableCreated { table: table_name })
        }
        Statement::CreateTableAs {
            name,
            query,
            if_not_exists,
        } => create_table_as(db, &name, *query, if_not_exists),
        Statement::DropTable { name, if_exists } => {
            let table_name = normalize_identifier(&name);
            db.acquire_write_lock(&table_name)?;
            if if_exists && !db.tables.contains_key(&table_name) {
                return Ok(QueryResult::TableDropped { table: table_name });
            }
            db.drop_table(&table_name)?;
            Ok(QueryResult::TableDropped { table: table_name })
        }
        Statement::Truncate { table } => truncate_table(db, &table),
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
        Statement::Insert { table, source } => match source {
            InsertSource::Values(rows) => insert_rows(db, &table, rows),
            InsertSource::Select(query) => insert_select(db, &table, *query),
        },
        Statement::Select(query) => execute_select(db, &query),
        Statement::Update {
            table,
            assignments,
            predicate,
        } => update(db, &table, &assignments, predicate.as_ref()),
        Statement::Delete { table, predicate } => delete(db, &table, predicate.as_ref()),
        Statement::CopyFrom { table, path } => copy_from(db, &table, &path),
        Statement::CopyTo { table, path } => copy_to(db, &table, &path),
        Statement::AlterTable { table, action } => match action {
            AlterAction::AddColumn { column } => alter_table_add_column(db, &table, column),
            AlterAction::RenameTable { new_name } => rename_table(db, &table, &new_name),
        },
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

fn insert_rows(db: &mut Database, table_name: &str, rows: Vec<Row>) -> Result<QueryResult> {
    if rows.is_empty() {
        return Ok(QueryResult::RowsInserted { count: 0 });
    }
    if rows.len() == 1 {
        let table_name = normalize_identifier(table_name);
        db.acquire_write_lock(&table_name)?;
        let row_id = {
            let table = db
                .tables
                .get_mut(&table_name)
                .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
            table.insert(rows.into_iter().next().expect("one row"))?
        };

        db.record_undo(UndoRecord::Insert {
            table: table_name,
            row_id,
        });
        return Ok(QueryResult::RowsInserted { count: 1 });
    }

    let table_name = normalize_identifier(table_name);
    db.acquire_write_lock(&table_name)?;
    let mut undo_records = Vec::new();
    {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        let mut inserted_ids = Vec::new();
        for row in rows {
            match table.insert(row) {
                Ok(row_id) => inserted_ids.push(row_id),
                Err(error) => {
                    for row_id in inserted_ids.iter().rev() {
                        let _ = table.delete_row(*row_id);
                    }
                    return Err(error);
                }
            }
        }
        for row_id in inserted_ids {
            undo_records.push(UndoRecord::Insert {
                table: table_name.clone(),
                row_id,
            });
        }
    }
    let inserted = undo_records.len();
    for undo in undo_records {
        db.record_undo(undo);
    }
    Ok(QueryResult::RowsInserted { count: inserted })
}

fn select_query(query: &SelectStatement) -> SelectQuery<'_> {
    SelectQuery {
        table: &query.table,
        alias: query.alias.as_deref(),
        joins: &query.joins,
        projection: &query.projection,
        predicate: query.predicate.as_ref(),
        group_by: &query.group_by,
        having: query.having.as_ref(),
        distinct: query.distinct,
        order_by: &query.order_by,
        limit: query.limit,
        offset: query.offset,
    }
}

fn insert_select(
    db: &mut Database,
    table_name: &str,
    query: SelectStatement,
) -> Result<QueryResult> {
    let selected = execute_select(db, &query)?;

    let QueryResult::Rows { rows, .. } = selected else {
        return Err(DbError::InvalidStatement(
            "INSERT SELECT source did not produce rows".into(),
        ));
    };

    let table_name = normalize_identifier(table_name);
    db.acquire_write_lock(&table_name)?;
    let expected = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?
        .schema
        .columns
        .len();

    if let Some(row) = rows.first()
        && row.len() != expected
    {
        return Err(DbError::ArityMismatch {
            expected,
            got: row.len(),
        });
    }

    let mut undo_records = Vec::new();
    {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        let mut inserted_ids = Vec::new();

        for row in rows {
            match table.insert(row) {
                Ok(row_id) => inserted_ids.push(row_id),
                Err(error) => {
                    for row_id in inserted_ids.iter().rev() {
                        let _ = table.delete_row(*row_id);
                    }
                    return Err(error);
                }
            }
        }

        for row_id in inserted_ids {
            undo_records.push(UndoRecord::Insert {
                table: table_name.clone(),
                row_id,
            });
        }
    }

    let inserted = undo_records.len();
    for undo in undo_records {
        db.record_undo(undo);
    }

    Ok(QueryResult::RowsInserted { count: inserted })
}

fn execute_select(db: &mut Database, query: &SelectStatement) -> Result<QueryResult> {
    if query.unions.is_empty() {
        return select(db, select_query(query));
    }

    let mut first = query.clone();
    first.unions.clear();
    first.order_by.clear();
    first.limit = None;
    first.offset = None;
    let selected = select(db, select_query(&first))?;
    let QueryResult::Rows { columns, mut rows } = selected else {
        return Err(DbError::InvalidStatement(
            "UNION source did not produce rows".into(),
        ));
    };

    for part in &query.unions {
        let next = select(db, select_query(&part.query))?;
        let QueryResult::Rows {
            columns: next_columns,
            rows: next_rows,
        } = next
        else {
            return Err(DbError::InvalidStatement(
                "UNION source did not produce rows".into(),
            ));
        };
        if next_columns.len() != columns.len() {
            return Err(DbError::ArityMismatch {
                expected: columns.len(),
                got: next_columns.len(),
            });
        }
        match part.op {
            SetOp::Union => {
                rows.extend(next_rows);
                if !part.all {
                    rows = distinct_rows(rows);
                }
            }
            SetOp::Except => {
                rows = except_rows(rows, next_rows);
            }
            SetOp::Intersect => {
                rows = intersect_rows(rows, next_rows);
            }
        }
    }

    let output_schema = QuerySchema::from_output_names(&columns);
    sort_rows(&output_schema, &mut rows, &query.order_by)?;
    apply_offset_limit(&mut rows, query.offset, query.limit);
    Ok(QueryResult::Rows { columns, rows })
}

fn select(db: &mut Database, query: SelectQuery<'_>) -> Result<QueryResult> {
    let left_name = normalize_identifier(query.table);
    db.acquire_read_lock(&left_name)?;
    for join in query.joins {
        db.acquire_read_lock(&normalize_identifier(&join.table))?;
    }

    let (schema, mut rows) = scan_and_join(db, &query)?;
    if !query.joins.is_empty() {
        let predicate = resolve_query_predicate(&schema, query.predicate, false)?;
        rows.retain(|row| matches_predicate(row, predicate.as_ref()));
    }

    let items = bind_projection(&schema, query.projection)?;
    let grouped = !query.group_by.is_empty()
        || items.iter().any(BoundSelectItem::is_aggregate)
        || query.having.is_some();

    let (columns, mut output_rows) = if grouped {
        let having = resolve_query_predicate(&schema, query.having, true)?;
        aggregate_rows(&schema, &rows, &items, query.group_by, having.as_ref())?
    } else {
        if query.distinct {
            let columns = items
                .iter()
                .map(BoundSelectItem::output_name)
                .collect::<Vec<_>>();
            let mut output_rows = rows
                .into_iter()
                .map(|row| project_row(&row, &items))
                .collect::<Vec<_>>();
            output_rows = distinct_rows(output_rows);
            let output_schema = QuerySchema::from_output_names(&columns);
            sort_rows(&output_schema, &mut output_rows, query.order_by)?;
            apply_offset_limit(&mut output_rows, query.offset, query.limit);
            return Ok(QueryResult::Rows {
                columns,
                rows: output_rows,
            });
        }

        sort_rows(&schema, &mut rows, query.order_by)?;
        apply_offset_limit(&mut rows, query.offset, query.limit);
        let columns = items
            .iter()
            .map(BoundSelectItem::output_name)
            .collect::<Vec<_>>();
        let output_rows = rows
            .into_iter()
            .map(|row| project_row(&row, &items))
            .collect();
        (columns, output_rows)
    };

    if grouped {
        if query.distinct {
            output_rows = distinct_rows(output_rows);
        }
        let output_schema = QuerySchema::from_output_names(&columns);
        sort_rows(&output_schema, &mut output_rows, query.order_by)?;
        apply_offset_limit(&mut output_rows, query.offset, query.limit);
    }

    Ok(QueryResult::Rows {
        columns,
        rows: output_rows,
    })
}

fn scan_and_join(db: &Database, query: &SelectQuery<'_>) -> Result<(QuerySchema, Vec<Row>)> {
    let left_name = normalize_identifier(query.table);
    let left_table = db
        .tables
        .get(&left_name)
        .ok_or_else(|| DbError::TableNotFound(left_name.clone()))?;
    let mut schema = QuerySchema::from_table(&left_name, query.alias, &left_table.schema);
    let mut rows = if query.joins.is_empty() {
        collect_matching_rows(left_table, query.predicate)?
    } else {
        left_table.rows().cloned().collect()
    };

    for join in query.joins {
        let right_name = normalize_identifier(&join.table);
        let right_table = db
            .tables
            .get(&right_name)
            .ok_or_else(|| DbError::TableNotFound(right_name.clone()))?;
        let right_schema =
            QuerySchema::from_table(&right_name, join.alias.as_deref(), &right_table.schema);
        let right_width = right_schema.columns.len();
        let left_width = schema.columns.len();
        let right_rows = right_table.rows().cloned().collect::<Vec<_>>();
        rows = if join.join_type == JoinType::Cross {
            cartesian_join(&rows, &right_rows)
        } else {
            let (left_index, right_index) =
                resolve_join_columns(&schema, &right_schema, &join.left, &join.right)?;
            match join.join_type {
                JoinType::Inner => hash_join(&rows, &right_rows, left_index, right_index),
                JoinType::Left => nested_loop_join(
                    &rows,
                    &right_rows,
                    left_index,
                    right_index,
                    JoinType::Left,
                    right_width,
                ),
                JoinType::Right => {
                    right_join(&rows, &right_rows, left_index, right_index, left_width)
                }
                JoinType::Cross => unreachable!("cross join handled above"),
            }
        };
        schema = schema.merge(right_schema);
    }

    Ok((schema, rows))
}

fn collect_matching_rows(table: &Table, predicate: Option<&Predicate>) -> Result<Vec<Row>> {
    let predicate = resolve_predicate(&table.schema, predicate)?;
    let mut row_ids = candidate_row_ids(table, predicate.as_ref());
    row_ids.retain(|row_id| {
        table
            .row(*row_id)
            .is_some_and(|row| matches_predicate(row, predicate.as_ref()))
    });
    Ok(row_ids
        .into_iter()
        .filter_map(|row_id| table.row(row_id).cloned())
        .collect())
}

fn resolve_join_columns(
    left: &QuerySchema,
    right: &QuerySchema,
    first: &str,
    second: &str,
) -> Result<(usize, usize)> {
    let first_on_left = left.resolve(first);
    let second_on_right = right.resolve(second);
    if let (Ok(left_index), Ok(right_index)) = (&first_on_left, &second_on_right) {
        return Ok((*left_index, *right_index));
    }

    let first_on_right = right.resolve(first);
    let second_on_left = left.resolve(second);
    if let (Ok(right_index), Ok(left_index)) = (&first_on_right, &second_on_left) {
        return Ok((*left_index, *right_index));
    }

    if first_on_left.is_err() && first_on_right.is_err() {
        return Err(DbError::ColumnNotFound(normalize_identifier(first)));
    }
    if second_on_left.is_err() && second_on_right.is_err() {
        return Err(DbError::ColumnNotFound(normalize_identifier(second)));
    }

    Err(DbError::InvalidStatement(format!(
        "cannot resolve join condition {first} = {second}"
    )))
}

fn cartesian_join(left_rows: &[Row], right_rows: &[Row]) -> Vec<Row> {
    let mut output = Vec::new();
    for left in left_rows {
        for right in right_rows {
            let mut row = left.clone();
            row.extend(right.iter().cloned());
            output.push(row);
        }
    }
    output
}

fn hash_join(
    left_rows: &[Row],
    right_rows: &[Row],
    left_index: usize,
    right_index: usize,
) -> Vec<Row> {
    let mut buckets: HashMap<Value, Vec<&Row>> = HashMap::new();
    for right in right_rows {
        if !right[right_index].is_null() {
            buckets
                .entry(right[right_index].clone())
                .or_default()
                .push(right);
        }
    }

    let mut output = Vec::new();
    for left in left_rows {
        if left[left_index].is_null() {
            continue;
        }
        if let Some(matches) = buckets.get(&left[left_index]) {
            for right in matches {
                let mut row = left.clone();
                row.extend(right.iter().cloned());
                output.push(row);
            }
        }
    }
    output
}

fn right_join(
    left_rows: &[Row],
    right_rows: &[Row],
    left_index: usize,
    right_index: usize,
    left_width: usize,
) -> Vec<Row> {
    let mut output = Vec::new();
    for right in right_rows {
        let mut matched = false;
        for left in left_rows {
            if join_values_equal(&left[left_index], &right[right_index]) {
                let mut row = left.clone();
                row.extend(right.iter().cloned());
                output.push(row);
                matched = true;
            }
        }
        if !matched {
            let mut row = vec![Value::Null; left_width];
            row.extend(right.iter().cloned());
            output.push(row);
        }
    }
    output
}

fn nested_loop_join(
    left_rows: &[Row],
    right_rows: &[Row],
    left_index: usize,
    right_index: usize,
    join_type: JoinType,
    right_width: usize,
) -> Vec<Row> {
    let mut output = Vec::new();

    for left in left_rows {
        let mut matched = false;
        for right in right_rows {
            if join_values_equal(&left[left_index], &right[right_index]) {
                let mut row = left.clone();
                row.extend(right.iter().cloned());
                output.push(row);
                matched = true;
            }
        }

        if !matched && join_type == JoinType::Left {
            let mut row = left.clone();
            row.extend(std::iter::repeat_n(Value::Null, right_width));
            output.push(row);
        }
    }

    output
}

fn join_values_equal(left: &Value, right: &Value) -> bool {
    if left.is_null() || right.is_null() {
        return false;
    }
    left == right
}

fn bind_projection(schema: &QuerySchema, projection: &Projection) -> Result<Vec<BoundSelectItem>> {
    match projection {
        Projection::All => Ok(schema
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| BoundSelectItem::Column {
                index,
                output_name: column.output_name.clone(),
            })
            .collect()),
        Projection::Columns(columns) => columns
            .iter()
            .map(|column| {
                let index = schema.resolve(column)?;
                Ok(BoundSelectItem::Column {
                    index,
                    output_name: column_output_name(column),
                })
            })
            .collect(),
        Projection::CountAll => Ok(vec![BoundSelectItem::CountAll]),
        Projection::Items(items) => items
            .iter()
            .map(|item| bind_select_item(schema, item))
            .collect(),
    }
}

fn bind_select_item(schema: &QuerySchema, item: &SelectItem) -> Result<BoundSelectItem> {
    match item {
        SelectItem::Column(column) => {
            let index = schema.resolve(column)?;
            Ok(BoundSelectItem::Column {
                index,
                output_name: column_output_name(column),
            })
        }
        SelectItem::CountAll => Ok(BoundSelectItem::CountAll),
        SelectItem::Count(column) => {
            let index = schema.resolve(column)?;
            Ok(BoundSelectItem::Count {
                index,
                output_name: "count".into(),
            })
        }
        SelectItem::Case {
            when,
            then_value,
            else_value,
        } => {
            let when = resolve_query_predicate_inner(schema, when, false)?;
            Ok(BoundSelectItem::Case {
                when: Box::new(when),
                then_value: then_value.clone(),
                else_value: else_value.clone(),
                output_name: "case".into(),
            })
        }
        SelectItem::Sum(column) => {
            bind_int_aggregate(schema, column, "sum", |index| BoundSelectItem::Sum {
                index,
                output_name: "sum".into(),
            })
        }
        SelectItem::Min(column) => {
            let index = schema.resolve(column)?;
            Ok(BoundSelectItem::Min {
                index,
                output_name: "min".into(),
            })
        }
        SelectItem::Max(column) => {
            let index = schema.resolve(column)?;
            Ok(BoundSelectItem::Max {
                index,
                output_name: "max".into(),
            })
        }
        SelectItem::Avg(column) => {
            bind_int_aggregate(schema, column, "avg", |index| BoundSelectItem::Avg {
                index,
                output_name: "avg".into(),
            })
        }
        SelectItem::Coalesce { column, fallback } => {
            let index = schema.resolve(column)?;
            Ok(BoundSelectItem::Coalesce {
                index,
                fallback: fallback.clone(),
                output_name: "coalesce".into(),
            })
        }
    }
}

fn bind_int_aggregate(
    schema: &QuerySchema,
    column: &str,
    func: &str,
    build: impl FnOnce(usize) -> BoundSelectItem,
) -> Result<BoundSelectItem> {
    let index = schema.resolve(column)?;
    if schema.columns[index].data_type != DataType::Int {
        return Err(DbError::TypeMismatch {
            column: format!("{func}({column})"),
            expected: "INT".into(),
            got: schema.columns[index].data_type.to_string(),
        });
    }
    Ok(build(index))
}

fn column_output_name(column: &str) -> String {
    normalize_identifier(column)
}

fn project_row(row: &Row, items: &[BoundSelectItem]) -> Row {
    items
        .iter()
        .map(|item| {
            if item.is_aggregate() {
                unreachable!("aggregates are projected by hash grouping")
            } else {
                eval_bound_item(std::slice::from_ref(row), item)
            }
        })
        .collect()
}

fn aggregate_rows(
    schema: &QuerySchema,
    rows: &[Row],
    items: &[BoundSelectItem],
    group_by: &[String],
    having: Option<&BoundPredicate>,
) -> Result<(Vec<String>, Vec<Row>)> {
    let group_indexes = group_by
        .iter()
        .map(|column| schema.resolve(column))
        .collect::<Result<Vec<_>>>()?;

    if group_indexes.is_empty() && items.iter().any(|item| !item.is_aggregate()) {
        return Err(DbError::InvalidStatement(
            "column must appear in GROUP BY or be an aggregate".into(),
        ));
    }

    for item in items {
        match item {
            BoundSelectItem::Column { index, output_name }
            | BoundSelectItem::Coalesce {
                index, output_name, ..
            } if !group_indexes.contains(index) => {
                return Err(DbError::InvalidStatement(format!(
                    "column {output_name} must appear in GROUP BY or be an aggregate"
                )));
            }
            _ => {}
        }
    }

    let columns = items
        .iter()
        .map(BoundSelectItem::output_name)
        .collect::<Vec<_>>();

    if group_indexes.is_empty() {
        if !matches_group(rows, having) {
            return Ok((columns, Vec::new()));
        }
        return Ok((columns, vec![aggregate_group(rows, items)]));
    }

    let mut groups: HashMap<Vec<Value>, Vec<Row>> = HashMap::new();
    for row in rows {
        let key = group_indexes
            .iter()
            .map(|index| row[*index].clone())
            .collect::<Vec<_>>();
        groups.entry(key).or_default().push(row.clone());
    }

    let mut keys = groups.keys().cloned().collect::<Vec<_>>();
    keys.sort_by(|left, right| cmp_value_lists(left, right));

    let output = keys
        .into_iter()
        .filter_map(|key| {
            let group_rows = groups.get(&key).expect("grouped rows exist");
            if matches_group(group_rows, having) {
                Some(aggregate_group(group_rows, items))
            } else {
                None
            }
        })
        .collect();

    Ok((columns, output))
}

fn aggregate_group(rows: &[Row], items: &[BoundSelectItem]) -> Row {
    items
        .iter()
        .map(|item| eval_bound_item(rows, item))
        .collect()
}

fn eval_bound_item(rows: &[Row], item: &BoundSelectItem) -> Value {
    match item {
        BoundSelectItem::Column { index, .. } => rows
            .first()
            .map(|row| row[*index].clone())
            .unwrap_or(Value::Null),
        BoundSelectItem::CountAll => Value::Int(rows.len() as i64),
        BoundSelectItem::Count { index, .. } => Value::Int(count_non_null(rows, *index)),
        BoundSelectItem::Sum { index, .. } => sum_int_column(rows, *index),
        BoundSelectItem::Min { index, .. } => extremum_column(rows, *index, Ordering::Less),
        BoundSelectItem::Max { index, .. } => extremum_column(rows, *index, Ordering::Greater),
        BoundSelectItem::Avg { index, .. } => avg_int_column(rows, *index),
        BoundSelectItem::Coalesce {
            index, fallback, ..
        } => {
            let value = rows
                .first()
                .map(|row| row[*index].clone())
                .unwrap_or(Value::Null);
            if value.is_null() {
                fallback.clone()
            } else {
                value
            }
        }
        BoundSelectItem::Case {
            when,
            then_value,
            else_value,
            ..
        } => {
            if matches_group(rows, Some(when.as_ref())) {
                then_value.clone()
            } else {
                else_value.clone()
            }
        }
    }
}

fn count_non_null(rows: &[Row], index: usize) -> i64 {
    rows.iter().filter(|row| !row[index].is_null()).count() as i64
}

fn sum_int_column(rows: &[Row], index: usize) -> Value {
    let mut total: Option<i64> = None;
    for row in rows {
        if let Value::Int(value) = row[index] {
            total = Some(total.unwrap_or(0) + value);
        }
    }
    match total {
        Some(total) => Value::Int(total),
        None => Value::Null,
    }
}

fn avg_int_column(rows: &[Row], index: usize) -> Value {
    let mut total: i64 = 0;
    let mut count: i64 = 0;
    for row in rows {
        if let Value::Int(value) = row[index] {
            total += value;
            count += 1;
        }
    }
    if count == 0 {
        Value::Null
    } else {
        Value::Int(total / count)
    }
}

fn extremum_column(rows: &[Row], index: usize, desired: Ordering) -> Value {
    let mut best: Option<Value> = None;
    for row in rows {
        let value = &row[index];
        if value.is_null() {
            continue;
        }
        match &best {
            None => best = Some(value.clone()),
            Some(current) => {
                if cmp_values(value, current) == desired {
                    best = Some(value.clone());
                }
            }
        }
    }
    best.unwrap_or(Value::Null)
}

fn distinct_rows(rows: Vec<Row>) -> Vec<Row> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for row in rows {
        if seen.insert(row.clone()) {
            unique.push(row);
        }
    }
    unique
}

fn except_rows(left: Vec<Row>, right: Vec<Row>) -> Vec<Row> {
    let right_set: HashSet<Row> = right.into_iter().collect();
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for row in left {
        if !right_set.contains(&row) && seen.insert(row.clone()) {
            output.push(row);
        }
    }
    output
}

fn intersect_rows(left: Vec<Row>, right: Vec<Row>) -> Vec<Row> {
    let right_set: HashSet<Row> = right.into_iter().collect();
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for row in left {
        if right_set.contains(&row) && seen.insert(row.clone()) {
            output.push(row);
        }
    }
    output
}

fn matches_group(rows: &[Row], predicate: Option<&BoundPredicate>) -> bool {
    match predicate {
        None => true,
        Some(BoundPredicate::Comparison { expr, op, value }) => {
            compare_values(&eval_bound_item(rows, expr), *op, value)
        }
        Some(BoundPredicate::Between {
            expr,
            low,
            high,
            negated,
        }) => {
            let left = eval_bound_item(rows, expr);
            let in_range = compare_values(&left, ComparisonOp::Gte, low)
                && compare_values(&left, ComparisonOp::Lte, high);
            if *negated {
                !left.is_null() && !in_range
            } else {
                in_range
            }
        }
        Some(BoundPredicate::InList {
            expr,
            values,
            negated,
        }) => {
            let left = eval_bound_item(rows, expr);
            in_list_matches(&left, values, *negated)
        }
        Some(BoundPredicate::IsNull { expr, negated }) => {
            let left = eval_bound_item(rows, expr);
            left.is_null() != *negated
        }
        Some(BoundPredicate::Like {
            column_index,
            pattern,
            escape,
        }) => match rows.first() {
            Some(row) => match &row[*column_index] {
                Value::Text(text) => like_matches(text, pattern, *escape),
                _ => false,
            },
            None => false,
        },
        Some(BoundPredicate::And(left, right)) => {
            matches_group(rows, Some(left)) && matches_group(rows, Some(right))
        }
        Some(BoundPredicate::Or(left, right)) => {
            matches_group(rows, Some(left)) || matches_group(rows, Some(right))
        }
    }
}

fn sort_rows(schema: &QuerySchema, rows: &mut [Row], order_by: &[OrderBy]) -> Result<()> {
    if order_by.is_empty() {
        return Ok(());
    }

    let keys = order_by
        .iter()
        .map(|key| {
            let index = resolve_order_column(schema, &key.column)?;
            Ok((index, key.direction))
        })
        .collect::<Result<Vec<_>>>()?;

    rows.sort_by(|left, right| {
        for (index, direction) in &keys {
            let ordering = cmp_values(&left[*index], &right[*index]);
            let ordering = match direction {
                SortDirection::Asc => ordering,
                SortDirection::Desc => ordering.reverse(),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });
    Ok(())
}

fn resolve_order_column(schema: &QuerySchema, column: &str) -> Result<usize> {
    if let Ok(position) = column.parse::<usize>()
        && position >= 1
        && position <= schema.columns.len()
    {
        return Ok(position - 1);
    }

    let column = normalize_identifier(column);
    if let Ok(index) = schema.resolve(&column) {
        return Ok(index);
    }

    let matches = matching_indexes(schema, |item| item.output_name == column);
    if let Ok(index) = unique_match(matches, &column) {
        return Ok(index);
    }

    schema.resolve(&column)
}

fn apply_offset_limit(rows: &mut Vec<Row>, offset: Option<usize>, limit: Option<usize>) {
    if let Some(offset) = offset {
        if offset >= rows.len() {
            rows.clear();
        } else {
            rows.drain(..offset);
        }
    }
    if let Some(limit) = limit {
        rows.truncate(limit);
    }
}

fn cmp_value_lists(left: &[Value], right: &[Value]) -> Ordering {
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        let ordering = cmp_values(left_value, right_value);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn cmp_values(left: &Value, right: &Value) -> Ordering {
    if let Some(ordering) = left.compare_same_type(right) {
        return ordering;
    }

    match (left.is_null(), right.is_null()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => left.type_name().cmp(right.type_name()),
    }
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

fn alter_table_add_column(
    db: &mut Database,
    table_name: &str,
    column: Column,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_write_lock(&table_name)?;
    let column_name = column.name.clone();
    let table = db
        .tables
        .get_mut(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    table.add_column(column)?;
    db.record_undo(UndoRecord::AddColumn {
        table: table_name.clone(),
    });
    Ok(QueryResult::ColumnAdded {
        table: table_name,
        column: column_name,
    })
}

fn rename_table(db: &mut Database, table_name: &str, new_name: &str) -> Result<QueryResult> {
    let from = normalize_identifier(table_name);
    let to = normalize_identifier(new_name);
    db.acquire_write_lock(&from)?;
    db.acquire_write_lock(&to)?;
    if from == to {
        return Ok(QueryResult::TableRenamed { from, to });
    }
    if db.tables.contains_key(&to) {
        return Err(DbError::TableExists(to));
    }
    let mut table = db
        .tables
        .remove(&from)
        .ok_or_else(|| DbError::TableNotFound(from.clone()))?;
    table.schema.name = to.clone();
    db.tables.insert(to.clone(), table);
    if let Some(stats) = db.stats.remove(&from) {
        db.stats.insert(to.clone(), stats);
    }
    db.record_undo(UndoRecord::RenameTable {
        from: from.clone(),
        to: to.clone(),
    });
    Ok(QueryResult::TableRenamed { from, to })
}

fn truncate_table(db: &mut Database, table_name: &str) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_write_lock(&table_name)?;
    let stored = {
        let table = db
            .tables
            .get(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        table.stored_rows().collect::<Vec<_>>()
    };
    let mut undo_records = Vec::new();
    {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        for row in &stored {
            table.delete_row(row.row_id)?;
            undo_records.push(UndoRecord::Delete {
                table: table_name.clone(),
                stored: row.clone(),
            });
        }
    }
    let count = undo_records.len();
    for undo in undo_records {
        db.record_undo(undo);
    }
    Ok(QueryResult::RowsDeleted { count })
}

fn create_table_as(
    db: &mut Database,
    name: &str,
    query: SelectStatement,
    if_not_exists: bool,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(name);
    if if_not_exists && db.tables.contains_key(&table_name) {
        return Ok(QueryResult::TableCreated { table: table_name });
    }

    let selected = execute_select(db, &query)?;
    let QueryResult::Rows { columns, rows } = selected else {
        return Err(DbError::InvalidStatement(
            "CREATE TABLE AS SELECT did not produce rows".into(),
        ));
    };

    let schema_columns = columns
        .iter()
        .enumerate()
        .map(|(index, column_name)| {
            let data_type = infer_column_type(&rows, index);
            Column::new(column_name, data_type)
        })
        .collect();
    let schema = TableSchema::new(table_name.clone(), schema_columns)?;
    db.acquire_write_lock(&table_name)?;
    db.create_table(schema)?;
    db.record_undo(UndoRecord::CreateTable {
        table: table_name.clone(),
    });

    let mut undo_records = Vec::new();
    {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        let mut inserted_ids = Vec::new();
        for row in rows {
            match table.insert(row) {
                Ok(row_id) => inserted_ids.push(row_id),
                Err(error) => {
                    for row_id in inserted_ids.iter().rev() {
                        let _ = table.delete_row(*row_id);
                    }
                    db.tables.remove(&table_name);
                    return Err(error);
                }
            }
        }
        for row_id in inserted_ids {
            undo_records.push(UndoRecord::Insert {
                table: table_name.clone(),
                row_id,
            });
        }
    }
    for undo in undo_records {
        db.record_undo(undo);
    }
    Ok(QueryResult::TableCreated { table: table_name })
}

fn infer_column_type(rows: &[Row], index: usize) -> crate::schema::DataType {
    for row in rows {
        match row.get(index) {
            Some(Value::Int(_)) => return DataType::Int,
            Some(Value::Text(_)) => return DataType::Text,
            Some(Value::Bool(_)) => return DataType::Bool,
            _ => {}
        }
    }
    DataType::Text
}

fn copy_to(db: &mut Database, table_name: &str, path: &str) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_read_lock(&table_name)?;
    let table = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;

    let mut output = String::new();
    let header = table
        .schema
        .column_names()
        .iter()
        .map(|name| csv_escape_field(name))
        .collect::<Vec<_>>()
        .join(",");
    output.push_str(&header);
    output.push('\n');

    let mut count = 0;
    for row in table.rows() {
        let line = row
            .iter()
            .map(csv_export_value)
            .collect::<Vec<_>>()
            .join(",");
        output.push_str(&line);
        output.push('\n');
        count += 1;
    }

    fs::write(path, output)?;
    Ok(QueryResult::RowsExported {
        path: path.to_string(),
        count,
    })
}

fn csv_export_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        other => csv_escape_field(&other.to_string()),
    }
}

fn csv_escape_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn copy_from(db: &mut Database, table_name: &str, path: &str) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_write_lock(&table_name)?;
    let schema = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?
        .schema
        .clone();

    let content = fs::read_to_string(path)?;
    let mut records = parse_csv(&content)?;
    if records.is_empty() {
        return Err(DbError::InvalidStatement(format!(
            "csv file {path} is empty"
        )));
    }

    let header = records
        .remove(0)
        .into_iter()
        .map(|name| normalize_identifier(&name))
        .collect::<Vec<_>>();
    let mapping = header
        .iter()
        .map(|name| {
            schema
                .column_index(name)
                .ok_or_else(|| DbError::ColumnNotFound(name.clone()))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut prepared = Vec::new();
    for record in records {
        if record.len() == 1 && record[0].is_empty() && header.len() != 1 {
            continue;
        }
        if record.len() != header.len() {
            return Err(DbError::InvalidStatement(format!(
                "csv row has {} field(s) but header has {}",
                record.len(),
                header.len()
            )));
        }

        let mut row = vec![Value::Null; schema.columns.len()];
        for (csv_index, &column_index) in mapping.iter().enumerate() {
            row[column_index] = csv_value(&schema.columns[column_index], &record[csv_index])?;
        }
        prepared.push(row);
    }

    let mut undo_records = Vec::new();
    {
        let table = db
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
        let mut inserted_ids = Vec::new();

        for row in prepared {
            match table.insert(row) {
                Ok(row_id) => inserted_ids.push(row_id),
                Err(error) => {
                    for row_id in inserted_ids.iter().rev() {
                        let _ = table.delete_row(*row_id);
                    }
                    return Err(error);
                }
            }
        }

        for row_id in inserted_ids {
            undo_records.push(UndoRecord::Insert {
                table: table_name.clone(),
                row_id,
            });
        }
    }

    let inserted = undo_records.len();
    for undo in undo_records {
        db.record_undo(undo);
    }

    Ok(QueryResult::RowsInserted { count: inserted })
}

fn parse_csv(content: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = content.chars().peekable();
    let mut in_quotes = false;
    let mut saw_field = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            saw_field = true;
            continue;
        }

        match ch {
            '"' => {
                in_quotes = true;
                saw_field = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                saw_field = true;
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|value| !value.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                saw_field = false;
            }
            '\r' => {}
            _ => {
                field.push(ch);
                saw_field = true;
            }
        }
    }

    if in_quotes {
        return Err(DbError::InvalidStatement(
            "unterminated quoted field in csv".into(),
        ));
    }

    if saw_field || !row.is_empty() {
        row.push(field);
        if row.iter().any(|value| !value.is_empty()) {
            rows.push(row);
        }
    }

    Ok(rows)
}

fn csv_value(column: &Column, raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        if column.nullable {
            return Ok(Value::Null);
        }
        return Err(DbError::ConstraintViolation(format!(
            "column {} cannot be NULL",
            column.name
        )));
    }

    match column.data_type {
        DataType::Int => {
            trimmed
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| DbError::TypeMismatch {
                    column: column.name.clone(),
                    expected: "INT".into(),
                    got: "TEXT".into(),
                })
        }
        DataType::Text => Ok(Value::Text(trimmed.to_string())),
        DataType::Bool => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Value::Bool(true)),
            "false" | "0" => Ok(Value::Bool(false)),
            _ => Err(DbError::TypeMismatch {
                column: column.name.clone(),
                expected: "BOOL".into(),
                got: "TEXT".into(),
            }),
        },
    }
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
        Statement::Select(query) => explain_select_statement(db, query)?,
        Statement::Insert {
            table,
            source: InsertSource::Values(_),
        } => format!("Insert\n  Table: {}", normalize_identifier(table)),
        Statement::Insert {
            table,
            source: InsertSource::Select(query),
        } => {
            let plan = explain_select_statement(db, query)?;
            let indented = plan
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Insert\n  Table: {}\n  Source:\n{indented}",
                normalize_identifier(table)
            )
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
        Statement::CopyFrom { table, path } => {
            format!(
                "CopyFrom\n  Table: {}\n  Path: {path}",
                normalize_identifier(table)
            )
        }
        Statement::CopyTo { table, path } => {
            format!(
                "CopyTo\n  Table: {}\n  Path: {path}",
                normalize_identifier(table)
            )
        }
        Statement::AlterTable { table, action } => match action {
            AlterAction::AddColumn { column } => format!(
                "AlterTable\n  Table: {}\n  Add: {} {}",
                normalize_identifier(table),
                column.name,
                column.data_type
            ),
            AlterAction::RenameTable { new_name } => format!(
                "AlterTable\n  Table: {}\n  RenameTo: {}",
                normalize_identifier(table),
                normalize_identifier(new_name)
            ),
        },
        Statement::CreateTableAs { name, .. } => {
            format!("CreateTableAs\n  Table: {}", normalize_identifier(name))
        }
        Statement::Truncate { table } => {
            format!("Truncate\n  Table: {}", normalize_identifier(table))
        }
        other => format!("{other:?}"),
    };

    Ok(QueryResult::Plan { plan })
}

fn explain_select_statement(db: &Database, query: &SelectStatement) -> Result<String> {
    let mut plan = explain_select(db, &select_query(query))?;
    for part in &query.unions {
        let kind = match (part.op, part.all) {
            (SetOp::Union, true) => "Union All",
            (SetOp::Union, false) => "Union",
            (SetOp::Except, _) => "Except",
            (SetOp::Intersect, _) => "Intersect",
        };
        let arm = explain_select(db, &select_query(&part.query))?;
        let indented = arm
            .lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        plan.push_str(&format!("\n  {kind}:\n{indented}"));
    }
    Ok(plan)
}

fn explain_select(db: &Database, query: &SelectQuery<'_>) -> Result<String> {
    let table_name = normalize_identifier(query.table);
    let table_ref = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    let projection = format_projection(query.projection);
    let table_label = match query.alias {
        Some(alias) => format!("{table_name} AS {alias}"),
        None => table_name.clone(),
    };
    let mut lines = vec![
        "Select".to_string(),
        format!("  Table: {table_label}"),
        format!("  Projection: {projection}"),
    ];

    for join in query.joins {
        let right_name = normalize_identifier(&join.table);
        if !db.tables.contains_key(&right_name) {
            return Err(DbError::TableNotFound(right_name));
        }
        let join_kind = match join.join_type {
            JoinType::Inner => "INNER",
            JoinType::Left => "LEFT",
            JoinType::Right => "RIGHT",
            JoinType::Cross => "CROSS",
        };
        let method = match join.join_type {
            JoinType::Inner => "hash join",
            JoinType::Cross => "cross product",
            JoinType::Left | JoinType::Right => "nested loop join",
        };
        let right_label = match &join.alias {
            Some(alias) => format!("{right_name} AS {alias}"),
            None => right_name,
        };
        if join.join_type == JoinType::Cross {
            lines.push(format!("  Join: {method} {join_kind} {right_label}"));
        } else {
            lines.push(format!(
                "  Join: {method} {join_kind} {right_label} ON {} = {}",
                join.left, join.right
            ));
        }
    }

    let access = if query.joins.is_empty() {
        let predicate = resolve_predicate(&table_ref.schema, query.predicate)?;
        describe_access_path(table_ref, predicate.as_ref(), db.stats.get(&table_name))
    } else if query
        .joins
        .iter()
        .any(|join| join.join_type == JoinType::Inner)
    {
        "hash join".into()
    } else {
        "nested loop join".into()
    };
    lines.push(format!("  Access: {access}"));

    if !query.group_by.is_empty() {
        lines.push(format!(
            "  Aggregate: hash group by {}",
            query.group_by.join(", ")
        ));
    } else if projection_has_aggregate(query.projection) {
        lines.push("  Aggregate: hash aggregate".into());
    }

    if let Some(having) = query.having {
        lines.push(format!("  Having: {}", format_predicate(having)));
    }

    if query.distinct {
        lines.push("  Distinct".into());
    }

    if !query.order_by.is_empty() {
        let keys = query
            .order_by
            .iter()
            .map(|key| format!("{} {:?}", normalize_identifier(&key.column), key.direction))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("  Sort: {keys}"));
    }

    if let Some(offset) = query.offset {
        lines.push(format!("  Offset: {offset}"));
    }

    if let Some(limit) = query.limit {
        lines.push(format!("  Limit: {limit}"));
    }

    Ok(lines.join("\n"))
}

fn format_projection(projection: &Projection) -> String {
    match projection {
        Projection::All => "*".into(),
        Projection::Columns(columns) => columns.join(", "),
        Projection::CountAll => "COUNT(*)".into(),
        Projection::Items(items) => items
            .iter()
            .map(format_select_item)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn projection_has_aggregate(projection: &Projection) -> bool {
    match projection {
        Projection::CountAll => true,
        Projection::Items(items) => items.iter().any(SelectItem::is_aggregate),
        Projection::All | Projection::Columns(_) => false,
    }
}

fn format_select_item(item: &SelectItem) -> String {
    match item {
        SelectItem::Column(column) => column.clone(),
        SelectItem::CountAll => "COUNT(*)".into(),
        SelectItem::Count(column) => format!("COUNT({column})"),
        SelectItem::Sum(column) => format!("SUM({column})"),
        SelectItem::Min(column) => format!("MIN({column})"),
        SelectItem::Max(column) => format!("MAX({column})"),
        SelectItem::Avg(column) => format!("AVG({column})"),
        SelectItem::Case {
            when,
            then_value,
            else_value,
        } => format!(
            "CASE WHEN {} THEN {then_value} ELSE {else_value} END",
            format_predicate(when)
        ),
        SelectItem::Coalesce { column, fallback } => {
            format!("COALESCE({column}, {fallback})")
        }
    }
}

fn format_predicate(predicate: &Predicate) -> String {
    match predicate {
        Predicate::Comparison { expr, op, value } => {
            format!("{} {} {value}", format_select_item(expr), format_op(*op))
        }
        Predicate::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let op = if *negated { "NOT BETWEEN" } else { "BETWEEN" };
            format!("{} {op} {low} AND {high}", format_select_item(expr))
        }
        Predicate::InList {
            expr,
            values,
            negated,
        } => {
            let list = values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let op = if *negated { "NOT IN" } else { "IN" };
            format!("{} {op} ({list})", format_select_item(expr))
        }
        Predicate::IsNull { expr, negated } => {
            let op = if *negated { "IS NOT NULL" } else { "IS NULL" };
            format!("{} {op}", format_select_item(expr))
        }
        Predicate::Like {
            column,
            pattern,
            escape,
        } => match escape {
            Some(escape) => format!("{column} LIKE '{pattern}' ESCAPE '{escape}'"),
            None => format!("{column} LIKE '{pattern}'"),
        },
        Predicate::And(left, right) => {
            format!(
                "({}) AND ({})",
                format_predicate(left),
                format_predicate(right)
            )
        }
        Predicate::Or(left, right) => {
            format!(
                "({}) OR ({})",
                format_predicate(left),
                format_predicate(right)
            )
        }
    }
}

fn format_op(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::Eq => "=",
        ComparisonOp::Ne => "!=",
        ComparisonOp::Lt => "<",
        ComparisonOp::Lte => "<=",
        ComparisonOp::Gt => ">",
        ComparisonOp::Gte => ">=",
    }
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
    if let Some((column_index, values)) = index_values(predicate)
        && let Some(index) = table.index_on_column(column_index)
    {
        if values.len() == 1 {
            return index.probe(&values[0]);
        }

        let mut seen = HashSet::new();
        let mut row_ids = Vec::new();
        for value in values {
            for row_id in index.probe(&value) {
                if seen.insert(row_id) {
                    row_ids.push(row_id);
                }
            }
        }
        return row_ids;
    }

    table.row_ids()
}

fn index_probe(predicate: Option<&BoundPredicate>) -> Option<(usize, Value)> {
    match index_values(predicate) {
        Some((column_index, values)) if values.len() == 1 => {
            Some((column_index, values.into_iter().next().expect("one key")))
        }
        _ => None,
    }
}

fn index_values(predicate: Option<&BoundPredicate>) -> Option<(usize, Vec<Value>)> {
    match predicate {
        Some(BoundPredicate::Comparison {
            expr: BoundSelectItem::Column { index, .. },
            op: ComparisonOp::Eq,
            value,
            ..
        }) => Some((*index, vec![value.clone()])),
        Some(BoundPredicate::InList {
            expr: BoundSelectItem::Column { index, .. },
            values,
            negated: false,
        }) => Some((*index, values.clone())),
        Some(BoundPredicate::And(left, right)) => {
            index_values(Some(left)).or_else(|| index_values(Some(right)))
        }
        _ => None,
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
    let query_schema = QuerySchema::from_table(&schema.name, None, schema);
    resolve_query_predicate(&query_schema, predicate, false)
}

fn resolve_query_predicate(
    schema: &QuerySchema,
    predicate: Option<&Predicate>,
    allow_aggregates: bool,
) -> Result<Option<BoundPredicate>> {
    let Some(predicate) = predicate else {
        return Ok(None);
    };
    resolve_query_predicate_inner(schema, predicate, allow_aggregates).map(Some)
}

fn resolve_query_predicate_inner(
    schema: &QuerySchema,
    predicate: &Predicate,
    allow_aggregates: bool,
) -> Result<BoundPredicate> {
    match predicate {
        Predicate::Comparison { expr, op, value } => {
            let expr = bind_predicate_expr(schema, expr, allow_aggregates)?;
            validate_expr_value(schema, &expr, value)?;
            Ok(BoundPredicate::Comparison {
                expr,
                op: *op,
                value: value.clone(),
            })
        }
        Predicate::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let expr = bind_predicate_expr(schema, expr, allow_aggregates)?;
            validate_expr_value(schema, &expr, low)?;
            validate_expr_value(schema, &expr, high)?;
            Ok(BoundPredicate::Between {
                expr,
                low: low.clone(),
                high: high.clone(),
                negated: *negated,
            })
        }
        Predicate::InList {
            expr,
            values,
            negated,
        } => {
            let expr = bind_predicate_expr(schema, expr, allow_aggregates)?;
            for value in values {
                validate_expr_value(schema, &expr, value)?;
            }
            Ok(BoundPredicate::InList {
                expr,
                values: values.clone(),
                negated: *negated,
            })
        }
        Predicate::IsNull { expr, negated } => {
            let expr = bind_predicate_expr(schema, expr, allow_aggregates)?;
            Ok(BoundPredicate::IsNull {
                expr,
                negated: *negated,
            })
        }
        Predicate::Like {
            column,
            pattern,
            escape,
        } => {
            let column = normalize_identifier(column);
            let index = schema.resolve(&column)?;
            if schema.columns[index].data_type != DataType::Text {
                return Err(DbError::TypeMismatch {
                    column: column.clone(),
                    expected: "TEXT".into(),
                    got: schema.columns[index].data_type.to_string(),
                });
            }
            Ok(BoundPredicate::Like {
                column_index: index,
                pattern: pattern.clone(),
                escape: *escape,
            })
        }
        Predicate::And(left, right) => Ok(BoundPredicate::And(
            Box::new(resolve_query_predicate_inner(
                schema,
                left,
                allow_aggregates,
            )?),
            Box::new(resolve_query_predicate_inner(
                schema,
                right,
                allow_aggregates,
            )?),
        )),
        Predicate::Or(left, right) => Ok(BoundPredicate::Or(
            Box::new(resolve_query_predicate_inner(
                schema,
                left,
                allow_aggregates,
            )?),
            Box::new(resolve_query_predicate_inner(
                schema,
                right,
                allow_aggregates,
            )?),
        )),
    }
}

fn bind_predicate_expr(
    schema: &QuerySchema,
    expr: &SelectItem,
    allow_aggregates: bool,
) -> Result<BoundSelectItem> {
    if !allow_aggregates && expr.is_aggregate() {
        return Err(DbError::InvalidStatement(
            "aggregates are not allowed in WHERE".into(),
        ));
    }
    bind_select_item(schema, expr)
}

fn validate_expr_value(schema: &QuerySchema, expr: &BoundSelectItem, value: &Value) -> Result<()> {
    if value.is_null() {
        return Ok(());
    }

    let (expected, column) = match expr {
        BoundSelectItem::Column { index, output_name } => (
            schema.columns[*index].data_type.clone(),
            output_name.clone(),
        ),
        BoundSelectItem::Min { index, .. } | BoundSelectItem::Max { index, .. } => {
            (schema.columns[*index].data_type.clone(), expr.output_name())
        }
        BoundSelectItem::CountAll
        | BoundSelectItem::Count { .. }
        | BoundSelectItem::Sum { .. }
        | BoundSelectItem::Avg { .. } => (DataType::Int, expr.output_name()),
        BoundSelectItem::Case { then_value, .. } => {
            if then_value.is_null() {
                return Ok(());
            }
            return if value.type_name() == then_value.type_name() {
                Ok(())
            } else {
                Err(DbError::TypeMismatch {
                    column: expr.output_name(),
                    expected: then_value.type_name().to_string(),
                    got: value.type_name().to_string(),
                })
            };
        }
        BoundSelectItem::Coalesce {
            index, fallback, ..
        } => {
            if fallback.is_null() {
                return Ok(());
            }
            (schema.columns[*index].data_type.clone(), expr.output_name())
        }
    };

    if !expected.accepts(value) {
        return Err(DbError::TypeMismatch {
            column,
            expected: expected.to_string(),
            got: value.type_name().to_string(),
        });
    }
    Ok(())
}

fn resolve_column(schema: &TableSchema, column: &str) -> Result<usize> {
    let column = normalize_identifier(column);
    if let Some(index) = schema.column_index(&column) {
        return Ok(index);
    }
    if let Some((_, name)) = column.split_once('.')
        && let Some(index) = schema.column_index(name)
    {
        return Ok(index);
    }
    Err(DbError::ColumnNotFound(column))
}

fn matches_predicate(row: &Row, predicate: Option<&BoundPredicate>) -> bool {
    matches_group(std::slice::from_ref(row), predicate)
}

fn in_list_matches(left: &Value, values: &[Value], negated: bool) -> bool {
    let matched = values
        .iter()
        .any(|value| compare_values(left, ComparisonOp::Eq, value));
    if !negated {
        return matched;
    }
    if left.is_null() || (!matched && values.iter().any(Value::is_null)) {
        return false;
    }
    !matched
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

fn like_matches(text: &str, pattern: &str, escape: Option<char>) -> bool {
    let text = text.chars().collect::<Vec<_>>();
    let pattern = pattern.chars().collect::<Vec<_>>();
    like_match_chars(&text, &pattern, escape)
}

fn like_match_chars(text: &[char], pattern: &[char], escape: Option<char>) -> bool {
    let mut text_index = 0;
    let mut pattern_index = 0;

    while pattern_index < pattern.len() {
        if escape == Some(pattern[pattern_index]) {
            pattern_index += 1;
            if pattern_index >= pattern.len() {
                return false;
            }
            if text_index >= text.len() || text[text_index] != pattern[pattern_index] {
                return false;
            }
            text_index += 1;
            pattern_index += 1;
            continue;
        }

        match pattern[pattern_index] {
            '%' => {
                pattern_index += 1;
                if pattern_index == pattern.len() {
                    return true;
                }
                while text_index <= text.len() {
                    if like_match_chars(&text[text_index..], &pattern[pattern_index..], escape) {
                        return true;
                    }
                    if text_index == text.len() {
                        break;
                    }
                    text_index += 1;
                }
                return false;
            }
            '_' => {
                if text_index >= text.len() {
                    return false;
                }
                text_index += 1;
                pattern_index += 1;
            }
            ch => {
                if text_index >= text.len() || text[text_index] != ch {
                    return false;
                }
                text_index += 1;
                pattern_index += 1;
            }
        }
    }

    text_index == text.len()
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
