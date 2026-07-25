use std::cmp::Ordering;

use crate::db::Database;
use crate::error::{DbError, Result};
use crate::optimizer::TableStats;
use crate::parser::{
    AggregateArg, AggregateCall, AggregateFunc, Assignment, ColumnRef, ComparisonOp, GroupExpr,
    HavingPredicate, Join, JoinKind, JoinTerm, OrderBy, Predicate, Projection, SelectItem,
    SortDirection, Statement,
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
            table_alias,
            joins,
            projection,
            predicate,
            group_by,
            having,
            order_by,
            limit,
        } => select(
            db,
            &table,
            table_alias.as_ref(),
            &joins,
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
    table_alias: Option<&String>,
    joins: &[Join],
    projection: &Projection,
    predicate: Option<&Predicate>,
    group_by: &[String],
    having: Option<&HavingPredicate>,
    order_by: Option<&OrderBy>,
    limit: Option<usize>,
) -> Result<QueryResult> {
    let table_name = normalize_identifier(table_name);
    db.acquire_read_lock(&table_name)?;
    for join in joins {
        db.acquire_read_lock(&normalize_identifier(&join.table))?;
    }

    // A join is materialised into a synthetic table; everything below then
    // runs over it exactly as it would over a base table.
    let joined = build_join_input(db, &table_name, table_alias, joins)?;
    let table = match &joined {
        Some((joined_table, _)) => joined_table,
        None => db
            .tables
            .get(&table_name)
            .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?,
    };
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

/* ---- joins -----------------------------------------------------------
 *
 * A join is executed by materialising it: the driving table and each joined
 * table are combined into one synthetic `Table` whose schema carries
 * qualified column names, and the existing filter / project / aggregate /
 * sort / limit machinery then runs over that unchanged.
 *
 * Materialising costs memory that a streaming operator tree would not, but it
 * buys something worth more here: every feature the engine already has works
 * on joined queries the day joins land, with no duplicated logic and nothing
 * silently unsupported. Streaming is the right next step, not the first one.
 *
 * Equality predicates use a hash join; anything else falls back to nested
 * loops. The planner picks per join and `EXPLAIN` reports which it used.
 */

/// How a single join was executed, for EXPLAIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinAlgorithm {
    Hash,
    NestedLoop,
}

impl JoinAlgorithm {
    fn name(self) -> &'static str {
        match self {
            JoinAlgorithm::Hash => "HashJoin",
            JoinAlgorithm::NestedLoop => "NestedLoopJoin",
        }
    }
}

/// Qualify a column name with a table/alias: `users` + `id` -> `users.id`.
fn qualify(prefix: &str, column: &str) -> String {
    format!("{prefix}.{column}")
}

/// Build the combined schema for a set of (prefix, schema) inputs. Every
/// column becomes nullable: a LEFT JOIN NULL-extends unmatched rows, so even
/// a NOT NULL column can legitimately hold NULL in the join output. Keys and
/// uniqueness are dropped for the same reason — they describe the base
/// tables, not the product.
fn combined_schema(parts: &[(String, &TableSchema)]) -> Result<TableSchema> {
    let mut columns = Vec::new();
    for (prefix, schema) in parts {
        for column in &schema.columns {
            let mut combined = Column::new(qualify(prefix, &column.name), column.data_type.clone());
            combined.nullable = true;
            columns.push(combined);
        }
    }
    TableSchema::new("<join>", columns)
}

/// Resolve a column reference against a combined schema. An unqualified name
/// matches on the part after the dot, and is an error if it is ambiguous —
/// the usual "column reference is ambiguous" that any SQL engine must give.
fn resolve_joined_column(schema: &TableSchema, name: &str) -> Result<usize> {
    let wanted = normalize_identifier(name);

    if wanted.contains('.') {
        return schema
            .column_index(&wanted)
            .ok_or(DbError::ColumnNotFound(wanted));
    }

    let mut found = None;
    for (index, column) in schema.columns.iter().enumerate() {
        let unqualified = column.name.split('.').next_back().unwrap_or(&column.name);
        if unqualified == wanted {
            if found.is_some() {
                return Err(DbError::InvalidStatement(format!(
                    "column reference {wanted} is ambiguous; qualify it with a table name"
                )));
            }
            found = Some(index);
        }
    }
    found.ok_or(DbError::ColumnNotFound(wanted))
}

/// One join term resolved to concrete column positions in the left (already
/// combined) and right (newly joined) row layouts.
struct BoundJoinTerm {
    left_index: usize,
    right_index: usize,
    op: ComparisonOp,
}

/// Execute the FROM clause, returning a materialised table plus a per-join
/// description of the algorithm used. Returns `None` when there are no joins,
/// so the single-table path stays exactly as it was.
fn build_join_input(
    db: &Database,
    table_name: &str,
    table_alias: Option<&String>,
    joins: &[Join],
) -> Result<Option<(Table, Vec<String>)>> {
    if joins.is_empty() {
        return Ok(None);
    }

    let base_name = normalize_identifier(table_name);
    let base = db
        .tables
        .get(&base_name)
        .ok_or_else(|| DbError::TableNotFound(base_name.clone()))?;
    let base_prefix = table_alias
        .map(|alias| normalize_identifier(alias))
        .unwrap_or_else(|| base_name.clone());

    // Start from the driving table, already qualified.
    let mut parts: Vec<(String, &TableSchema)> = vec![(base_prefix.clone(), &base.schema)];
    let mut schema = combined_schema(&parts)?;
    let mut rows: Vec<Row> = base.rows().cloned().collect();
    let mut plan = Vec::new();

    for join in joins {
        let right_name = normalize_identifier(&join.table);
        let right = db
            .tables
            .get(&right_name)
            .ok_or_else(|| DbError::TableNotFound(right_name.clone()))?;
        let right_prefix = join
            .alias
            .as_ref()
            .map(|alias| normalize_identifier(alias))
            .unwrap_or_else(|| right_name.clone());

        if parts.iter().any(|(prefix, _)| prefix == &right_prefix) {
            return Err(DbError::InvalidStatement(format!(
                "table name {right_prefix} appears twice; give one of them an alias"
            )));
        }

        // Resolve the ON terms: each side may name a column from either input,
        // so try the left layout first and fall back to the right.
        let right_schema = &right.schema;
        let mut terms = Vec::new();
        if let Some(condition) = &join.on {
            for term in &condition.terms {
                let bound = bind_join_term(&schema, right_schema, &right_prefix, term)?;
                terms.push(bound);
            }
        }

        // A hash join needs at least one equality; otherwise nested loops.
        let algorithm = if terms.iter().any(|t| t.op == ComparisonOp::Eq) {
            JoinAlgorithm::Hash
        } else {
            JoinAlgorithm::NestedLoop
        };

        let right_rows: Vec<Row> = right.rows().cloned().collect();
        let right_width = right_schema.columns.len();

        rows = match algorithm {
            JoinAlgorithm::Hash => hash_join(&rows, &right_rows, &terms, join.kind, right_width),
            JoinAlgorithm::NestedLoop => {
                nested_loop_join(&rows, &right_rows, &terms, join.kind, right_width)
            }
        };

        plan.push(format!(
            "{}({} {}, on={})",
            algorithm.name(),
            match join.kind {
                JoinKind::Inner => "INNER",
                JoinKind::Left => "LEFT",
                JoinKind::Cross => "CROSS",
            },
            right_prefix,
            join.on
                .as_ref()
                .map(|c| c
                    .terms
                    .iter()
                    .map(|t| format!("{} {}", t.left.display(), t.right.display()))
                    .collect::<Vec<_>>()
                    .join(" AND "))
                .unwrap_or_else(|| "-".into())
        ));

        parts.push((right_prefix, right_schema));
        schema = combined_schema(&parts)?;
    }

    // Load the joined rows into a table so the rest of the engine can treat
    // this exactly like a base table.
    let mut joined = Table::new(schema);
    for row in rows {
        joined.insert(row)?;
    }

    Ok(Some((joined, plan)))
}

/// Bind one ON term. Either side may reference the left (combined) input or
/// the right (newly joined) table; the term is normalised so `left_index`
/// always refers to the left layout.
fn bind_join_term(
    left_schema: &TableSchema,
    right_schema: &TableSchema,
    right_prefix: &str,
    term: &JoinTerm,
) -> Result<BoundJoinTerm> {
    let in_right = |reference: &ColumnRef| -> Option<usize> {
        if let Some(qualifier) = &reference.qualifier
            && normalize_identifier(qualifier) != right_prefix
        {
            return None;
        }
        right_schema.column_index(&normalize_identifier(&reference.name))
    };

    let left_first = resolve_joined_column(left_schema, &term.left.display()).ok();
    let right_first = in_right(&term.left);

    match (left_first, right_first) {
        // `left.col op right.col`
        (Some(left_index), _) => {
            let right_index = in_right(&term.right).ok_or_else(|| {
                DbError::ColumnNotFound(format!(
                    "{} (not a column of {right_prefix})",
                    term.right.display()
                ))
            })?;
            Ok(BoundJoinTerm {
                left_index,
                right_index,
                op: term.op,
            })
        }
        // `right.col op left.col` — flip so left_index is always the left side.
        (None, Some(right_index)) => {
            let left_index = resolve_joined_column(left_schema, &term.right.display())?;
            Ok(BoundJoinTerm {
                left_index,
                right_index,
                op: flip_op(term.op),
            })
        }
        (None, None) => Err(DbError::ColumnNotFound(term.left.display())),
    }
}

/// Reverse a comparison so `a < b` becomes `b > a`.
fn flip_op(op: ComparisonOp) -> ComparisonOp {
    match op {
        ComparisonOp::Eq => ComparisonOp::Eq,
        ComparisonOp::Ne => ComparisonOp::Ne,
        ComparisonOp::Lt => ComparisonOp::Gt,
        ComparisonOp::Lte => ComparisonOp::Gte,
        ComparisonOp::Gt => ComparisonOp::Lt,
        ComparisonOp::Gte => ComparisonOp::Lte,
    }
}

fn join_terms_match(left: &Row, right: &Row, terms: &[BoundJoinTerm]) -> bool {
    terms
        .iter()
        .all(|term| compare_values(&left[term.left_index], term.op, &right[term.right_index]))
}

fn null_extend(left: &Row, width: usize) -> Row {
    let mut row = left.clone();
    row.extend(std::iter::repeat_n(Value::Null, width));
    row
}

fn concat_rows(left: &Row, right: &Row) -> Row {
    let mut row = left.clone();
    row.extend(right.iter().cloned());
    row
}

/// Nested loops: the general case. Handles any comparison, including none at
/// all (a cross join).
fn nested_loop_join(
    left_rows: &[Row],
    right_rows: &[Row],
    terms: &[BoundJoinTerm],
    kind: JoinKind,
    right_width: usize,
) -> Vec<Row> {
    let mut out = Vec::new();
    for left in left_rows {
        let mut matched = false;
        for right in right_rows {
            if terms.is_empty() || join_terms_match(left, right, terms) {
                out.push(concat_rows(left, right));
                matched = true;
            }
        }
        if !matched && kind == JoinKind::Left {
            out.push(null_extend(left, right_width));
        }
    }
    out
}

/// Hash join on the equality terms: build a table from the right input keyed
/// by those columns, then probe it once per left row. That turns the
/// quadratic scan into one pass over each side, which is the whole point.
/// Any non-equality terms are re-checked on each candidate pair.
fn hash_join(
    left_rows: &[Row],
    right_rows: &[Row],
    terms: &[BoundJoinTerm],
    kind: JoinKind,
    right_width: usize,
) -> Vec<Row> {
    use std::collections::HashMap;

    let equalities: Vec<&BoundJoinTerm> =
        terms.iter().filter(|t| t.op == ComparisonOp::Eq).collect();
    let residual: Vec<&BoundJoinTerm> = terms.iter().filter(|t| t.op != ComparisonOp::Eq).collect();

    // NULL never equals anything, so a row with a NULL key can never match and
    // is left out of the hash table entirely.
    let mut buckets: HashMap<Vec<Value>, Vec<&Row>> = HashMap::new();
    for right in right_rows {
        let key: Vec<Value> = equalities
            .iter()
            .map(|t| right[t.right_index].clone())
            .collect();
        if key.iter().any(|value| value.is_null()) {
            continue;
        }
        buckets.entry(key).or_default().push(right);
    }

    let mut out = Vec::new();
    for left in left_rows {
        let key: Vec<Value> = equalities
            .iter()
            .map(|t| left[t.left_index].clone())
            .collect();

        let mut matched = false;
        if !key.iter().any(|value| value.is_null())
            && let Some(candidates) = buckets.get(&key)
        {
            for right in candidates {
                if residual
                    .iter()
                    .all(|t| compare_values(&left[t.left_index], t.op, &right[t.right_index]))
                {
                    out.push(concat_rows(left, right));
                    matched = true;
                }
            }
        }

        if !matched && kind == JoinKind::Left {
            out.push(null_extend(left, right_width));
        }
    }
    out
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
            table_alias,
            joins,
            projection,
            predicate,
            group_by,
            having,
            order_by,
            limit,
        } => explain_select(
            db,
            table,
            table_alias.as_ref(),
            joins,
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
    table_alias: Option<&String>,
    joins: &[Join],
    projection: &Projection,
    predicate: Option<&Predicate>,
    group_by: &[String],
    having: Option<&HavingPredicate>,
    order_by: Option<&OrderBy>,
    limit: Option<usize>,
) -> Result<String> {
    let table_name = normalize_identifier(table);

    // Planning a join means building it, since the combined schema is what
    // the predicate and projection resolve against.
    let joined = build_join_input(db, &table_name, table_alias, joins)?;
    let (table, join_plan) = match &joined {
        Some((joined_table, plan)) => (joined_table, plan.clone()),
        None => (
            db.tables
                .get(&table_name)
                .ok_or_else(|| DbError::TableNotFound(table_name.clone()))?,
            Vec::new(),
        ),
    };
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
    ];

    for step in &join_plan {
        lines.push(format!("  Join: {step}"));
    }

    lines.push(format!(
        "  Access: {}",
        describe_access_path(table, predicate.as_ref(), db.stats.get(&table_name))
    ));

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
    let name = normalize_identifier(column);
    // Exact match first: this is the only case for a base table, whose column
    // names never contain a dot.
    if let Some(index) = schema.column_index(&name) {
        return Ok(index);
    }
    // Otherwise the schema is a join's combined schema, whose names are
    // qualified — fall back to matching on the unqualified part.
    resolve_joined_column(schema, &name)
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
