use std::cmp::Ordering;

use crate::db::Database;
use crate::error::{DbError, Result};
use crate::optimizer::TableStats;
use crate::parser::{
    AggregateArg, AggregateCall, AggregateFunc, Assignment, ComparisonOp, GroupExpr,
    HavingPredicate, OrderBy, Predicate, Projection, SelectItem, SortDirection, Statement,
};
use crate::row::Row;
use crate::schema::{DataType, TableSchema, normalize_identifier};
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
            group_by,
            having,
            order_by,
            limit,
        } => select(
            db,
            &table,
            &projection,
            predicate.as_ref(),
            &group_by,
            having.as_ref(),
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

#[allow(clippy::too_many_arguments)]
fn select(
    db: &mut Database,
    table_name: &str,
    projection: &Projection,
    predicate: Option<&Predicate>,
    group_by: &[String],
    having: Option<&HavingPredicate>,
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

    // Grouping and aggregation take a separate path: they collapse the
    // filtered rows into one row per group rather than projecting each row.
    if let Projection::Aggregate(items) = projection {
        return aggregate(table, items, group_by, having, order_by, limit, &row_ids);
    }
    if !group_by.is_empty() || having.is_some() {
        return Err(DbError::Parse(
            "GROUP BY / HAVING require an aggregate or grouped projection".into(),
        ));
    }

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

/// A `GROUP BY` grouping column resolved against the schema.
struct ResolvedGroup {
    /// The column's position in a row.
    row_index: usize,
    /// The column's normalized name (its default output label).
    name: String,
}

/// An aggregate call resolved against the schema and ready to compute.
struct ResolvedAgg {
    func: AggregateFunc,
    /// `None` for `COUNT(*)`; otherwise the argument column's row index.
    column: Option<usize>,
    distinct: bool,
}

impl ResolvedAgg {
    fn resolve(schema: &TableSchema, call: &AggregateCall) -> Result<Self> {
        let column = match &call.arg {
            AggregateArg::Star => None,
            AggregateArg::Column(name) => Some(resolve_column(schema, name)?),
        };

        // SUM and AVG are only meaningful over a numeric column; catching that
        // here yields a clear error rather than a per-row type surprise.
        if matches!(call.func, AggregateFunc::Sum | AggregateFunc::Avg)
            && let Some(index) = column
            && schema.columns[index].data_type != DataType::Int
        {
            return Err(DbError::TypeMismatch {
                column: schema.columns[index].name.clone(),
                expected: "INT".into(),
                got: schema.columns[index].data_type.to_string(),
            });
        }

        Ok(ResolvedAgg {
            func: call.func,
            column,
            distinct: call.distinct,
        })
    }

    /// Compute this aggregate over one group's rows, applying SQL NULL rules:
    /// `COUNT(*)` counts every row; every other aggregate skips NULLs, and an
    /// aggregate over no (non-NULL) values is NULL — except `COUNT`, which is 0.
    fn compute(&self, table: &Table, rows: &[RowId]) -> Result<Value> {
        // COUNT(*) is the only aggregate that sees NULL rows.
        if self.func == AggregateFunc::Count && self.column.is_none() {
            return Ok(Value::Int(rows.len() as i64));
        }

        let column = self.column.expect("non-COUNT(*) aggregates have a column");
        let mut values: Vec<Value> = rows
            .iter()
            .filter_map(|id| table.row(*id))
            .map(|row| row[column].clone())
            .filter(|value| !value.is_null())
            .collect();

        if self.distinct {
            values = dedupe_values(values);
        }

        match self.func {
            AggregateFunc::Count => Ok(Value::Int(values.len() as i64)),
            AggregateFunc::Sum => sum_values(&values),
            AggregateFunc::Avg => avg_values(&values),
            AggregateFunc::Min => Ok(min_max_values(&values, Ordering::Less)),
            AggregateFunc::Max => Ok(min_max_values(&values, Ordering::Greater)),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn aggregate(
    table: &Table,
    items: &[SelectItem],
    group_by: &[String],
    having: Option<&HavingPredicate>,
    order_by: Option<&OrderBy>,
    limit: Option<usize>,
    row_ids: &[RowId],
) -> Result<QueryResult> {
    let schema = &table.schema;

    let groups_cols: Vec<ResolvedGroup> = group_by
        .iter()
        .map(|name| {
            let name = normalize_identifier(name);
            Ok(ResolvedGroup {
                row_index: resolve_column(schema, &name)?,
                name,
            })
        })
        .collect::<Result<_>>()?;

    // Resolve each output item. A passthrough column is only legal when it is
    // one of the grouping columns; otherwise its value is not determined by
    // the group (the classic "must appear in GROUP BY" error).
    enum OutputCol {
        Group(usize), // position within the group key
        Agg(ResolvedAgg),
    }
    let mut output_names = Vec::with_capacity(items.len());
    let mut output_cols = Vec::with_capacity(items.len());

    for item in items {
        match item {
            SelectItem::Column { name, alias } => {
                let column = normalize_identifier(name);
                let position = groups_cols
                    .iter()
                    .position(|group| group.name == column)
                    .ok_or_else(|| {
                        DbError::InvalidStatement(format!(
                            "column {column} must appear in GROUP BY or an aggregate"
                        ))
                    })?;
                output_names.push(alias.clone().unwrap_or(column));
                output_cols.push(OutputCol::Group(position));
            }
            SelectItem::Aggregate { call, alias } => {
                let resolved = ResolvedAgg::resolve(schema, call)?;
                output_names.push(alias.clone().unwrap_or_else(|| default_agg_name(call)));
                output_cols.push(OutputCol::Agg(resolved));
            }
        }
    }

    // Partition the filtered rows into groups, preserving first-seen order so
    // output is deterministic without an explicit ORDER BY.
    let group_indices: Vec<usize> = groups_cols.iter().map(|group| group.row_index).collect();
    let grouped = partition_into_groups(table, &group_indices, row_ids);

    // Produce one output row per group.
    let mut out_rows: Vec<(Vec<Value>, Row)> = Vec::with_capacity(grouped.len());
    for (key, members) in &grouped {
        // HAVING is evaluated per group, over grouping columns and aggregates.
        if let Some(having) = having
            && !eval_having(having, schema, &groups_cols, key, table, members)?
        {
            continue;
        }

        let mut row = Row::with_capacity(output_cols.len());
        for col in &output_cols {
            match col {
                OutputCol::Group(position) => row.push(key[*position].clone()),
                OutputCol::Agg(agg) => row.push(agg.compute(table, members)?),
            }
        }
        out_rows.push((key.clone(), row));
    }

    // ORDER BY on the aggregated output references an output column by name.
    if let Some(order_by) = order_by {
        let order_column = normalize_identifier(&order_by.column);
        let position = output_names
            .iter()
            .position(|name| name == &order_column)
            .ok_or_else(|| {
                DbError::ColumnNotFound(format!("{order_column} (not in the SELECT list)"))
            })?;
        out_rows.sort_by(|left, right| {
            let ordering = left.1[position]
                .compare_same_type(&right.1[position])
                .unwrap_or(Ordering::Equal);
            match order_by.direction {
                SortDirection::Asc => ordering,
                SortDirection::Desc => ordering.reverse(),
            }
        });
    }

    let mut rows: Vec<Row> = out_rows.into_iter().map(|(_, row)| row).collect();
    if let Some(limit) = limit {
        rows.truncate(limit);
    }

    Ok(QueryResult::Rows {
        columns: output_names,
        rows,
    })
}

/// Group `row_ids` by the tuple of values in `group_indices`, keeping groups
/// in first-seen order. An empty `group_indices` collapses everything into a
/// single group — which, per SQL, exists even when there are no rows, so an
/// aggregate with no `GROUP BY` still yields one output row.
fn partition_into_groups(
    table: &Table,
    group_indices: &[usize],
    row_ids: &[RowId],
) -> Vec<(Vec<Value>, Vec<RowId>)> {
    use std::collections::HashMap;

    if group_indices.is_empty() {
        return vec![(Vec::new(), row_ids.to_vec())];
    }

    let mut order: Vec<Vec<Value>> = Vec::new();
    let mut buckets: HashMap<Vec<Value>, Vec<RowId>> = HashMap::new();

    for &id in row_ids {
        let Some(row) = table.row(id) else { continue };
        let key: Vec<Value> = group_indices.iter().map(|&i| row[i].clone()).collect();
        if !buckets.contains_key(&key) {
            order.push(key.clone());
        }
        buckets.entry(key).or_default().push(id);
    }

    order
        .into_iter()
        .map(|key| {
            let members = buckets.remove(&key).unwrap_or_default();
            (key, members)
        })
        .collect()
}

fn default_agg_name(call: &AggregateCall) -> String {
    match &call.arg {
        // Matches the bare-COUNT(*) fast path so both name their column "count".
        AggregateArg::Star => "count".into(),
        AggregateArg::Column(column) => {
            format!("{}_{}", call.func.name(), normalize_identifier(column))
        }
    }
}

fn dedupe_values(values: Vec<Value>) -> Vec<Value> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn sum_values(values: &[Value]) -> Result<Value> {
    if values.is_empty() {
        return Ok(Value::Null); // SUM over no rows is NULL, not 0.
    }
    let mut total: i64 = 0;
    for value in values {
        match value {
            Value::Int(n) => {
                total = total.checked_add(*n).ok_or_else(|| {
                    DbError::InvalidStatement("SUM overflowed a 64-bit integer".into())
                })?;
            }
            other => {
                return Err(DbError::TypeMismatch {
                    column: "SUM argument".into(),
                    expected: "INT".into(),
                    got: other.type_name().into(),
                });
            }
        }
    }
    Ok(Value::Int(total))
}

/// AVG returns integer division truncated toward zero: the value model has no
/// floating-point type, so there is nowhere to put a fractional average.
fn avg_values(values: &[Value]) -> Result<Value> {
    if values.is_empty() {
        return Ok(Value::Null);
    }
    let Value::Int(total) = sum_values(values)? else {
        return Ok(Value::Null);
    };
    Ok(Value::Int(total / values.len() as i64))
}

fn min_max_values(values: &[Value], want: Ordering) -> Value {
    let mut best: Option<&Value> = None;
    for value in values {
        match best {
            None => best = Some(value),
            Some(current) => {
                if value.compare_same_type(current) == Some(want) {
                    best = Some(value);
                }
            }
        }
    }
    best.cloned().unwrap_or(Value::Null)
}

fn eval_having(
    having: &HavingPredicate,
    schema: &TableSchema,
    groups_cols: &[ResolvedGroup],
    key: &[Value],
    table: &Table,
    members: &[RowId],
) -> Result<bool> {
    match having {
        HavingPredicate::And(left, right) => {
            Ok(eval_having(left, schema, groups_cols, key, table, members)?
                && eval_having(right, schema, groups_cols, key, table, members)?)
        }
        HavingPredicate::Or(left, right) => {
            Ok(eval_having(left, schema, groups_cols, key, table, members)?
                || eval_having(right, schema, groups_cols, key, table, members)?)
        }
        HavingPredicate::Comparison { left, op, value } => {
            let actual = eval_group_expr(left, schema, groups_cols, key, table, members)?;
            Ok(compare_values(&actual, *op, value))
        }
    }
}

fn eval_group_expr(
    expr: &GroupExpr,
    schema: &TableSchema,
    groups_cols: &[ResolvedGroup],
    key: &[Value],
    table: &Table,
    members: &[RowId],
) -> Result<Value> {
    match expr {
        GroupExpr::Column(name) => {
            let column = normalize_identifier(name);
            let position = groups_cols
                .iter()
                .position(|group| group.name == column)
                .ok_or_else(|| {
                    DbError::InvalidStatement(format!(
                        "HAVING column {column} must appear in GROUP BY"
                    ))
                })?;
            Ok(key[position].clone())
        }
        GroupExpr::Aggregate(call) => {
            let resolved = ResolvedAgg::resolve(schema, call)?;
            resolved.compute(table, members)
        }
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
            group_by,
            having,
            order_by,
            limit,
        } => explain_select(
            db,
            table,
            projection,
            predicate.as_ref(),
            group_by,
            having.as_ref(),
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

#[allow(clippy::too_many_arguments)]
fn explain_select(
    db: &Database,
    table: &str,
    projection: &Projection,
    predicate: Option<&Predicate>,
    group_by: &[String],
    having: Option<&HavingPredicate>,
    order_by: Option<&OrderBy>,
    limit: Option<usize>,
) -> Result<String> {
    let table_name = normalize_identifier(table);
    let table = db
        .tables
        .get(&table_name)
        .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?;
    let predicate = resolve_predicate(&table.schema, predicate)?;
    let is_aggregate = matches!(projection, Projection::Aggregate(_));
    let projection_text = match projection {
        Projection::All => "*".into(),
        Projection::Columns(columns) => columns.join(", "),
        Projection::CountAll => "COUNT(*)".into(),
        Projection::Aggregate(items) => describe_aggregate_projection(items),
    };
    let mut lines = vec![
        "Select".to_string(),
        format!("  Table: {table_name}"),
        format!("  Projection: {projection_text}"),
        format!(
            "  Access: {}",
            describe_access_path(table, predicate.as_ref(), db.stats.get(&table_name))
        ),
    ];

    if is_aggregate {
        let keys: Vec<String> = group_by.iter().map(|c| normalize_identifier(c)).collect();
        lines.push(format!(
            "  Aggregate: HashAggregate group_by=[{}]",
            keys.join(", ")
        ));
    }

    if having.is_some() {
        lines.push("  Having: filter groups".to_string());
    }

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

fn describe_aggregate_projection(items: &[SelectItem]) -> String {
    items
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, .. } => normalize_identifier(name),
            SelectItem::Aggregate { call, .. } => {
                let arg = match &call.arg {
                    AggregateArg::Star => "*".to_string(),
                    AggregateArg::Column(column) => normalize_identifier(column),
                };
                let distinct = if call.distinct { "DISTINCT " } else { "" };
                format!("{}({distinct}{arg})", call.func.name().to_uppercase())
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
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
        // Aggregated projections are handled entirely by `aggregate()`, which
        // never routes through this per-row projection helper.
        Projection::Aggregate(_) => Err(DbError::InvalidStatement(
            "aggregate projection cannot be resolved as plain columns".into(),
        )),
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
