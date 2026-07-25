use crate::error::{DbError, Result};
use crate::schema::{Column, DataType, normalize_identifier};
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<Column>,
    },
    CreateIndex {
        name: String,
        table: String,
        column: String,
        unique: bool,
    },
    DropTable {
        name: String,
    },
    DropIndex {
        name: String,
    },
    Insert {
        table: String,
        values: Vec<Value>,
    },
    Select {
        /// The driving table. Kept as a plain name so every existing caller
        /// and test that reads `table` still works.
        table: String,
        /// Alias for the driving table, if `FROM t AS x` was written.
        table_alias: Option<String>,
        /// Tables joined onto it, in written order.
        joins: Vec<Join>,
        projection: Projection,
        predicate: Option<Predicate>,
        group_by: Vec<String>,
        having: Option<HavingPredicate>,
        order_by: Option<OrderBy>,
        limit: Option<usize>,
    },
    Update {
        table: String,
        assignments: Vec<Assignment>,
        predicate: Option<Predicate>,
    },
    Delete {
        table: String,
        predicate: Option<Predicate>,
    },
    Explain(Box<Statement>),
    Begin,
    Commit,
    Rollback,
    Analyze {
        table: Option<String>,
    },
    Checkpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Projection {
    All,
    Columns(Vec<String>),
    /// A bare `SELECT COUNT(*)` with no GROUP BY. Kept as its own variant so
    /// the common whole-table row count stays a trivial path; anything richer
    /// (grouping, other aggregates, a mix of columns and aggregates) is an
    /// [`Projection::Aggregate`].
    CountAll,
    /// A grouped/aggregated projection: a list of grouping columns and/or
    /// aggregate calls. Produced whenever the query has a `GROUP BY` or the
    /// select list contains an aggregate function.
    Aggregate(Vec<SelectItem>),
}

/// One joined table: how to join it, and on what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Join {
    pub table: String,
    pub alias: Option<String>,
    pub kind: JoinKind,
    /// `None` for CROSS JOIN and comma joins, which pair every row.
    pub on: Option<JoinCondition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    /// Keeps unmatched left rows, NULL-extending the right side.
    Left,
    Cross,
}

/// A join predicate. Restricted to a conjunction of column comparisons, which
/// is what an equi-join needs and what the hash join can exploit; anything
/// richer belongs in `WHERE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinCondition {
    pub terms: Vec<JoinTerm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinTerm {
    pub left: ColumnRef,
    pub op: ComparisonOp,
    pub right: ColumnRef,
}

/// A possibly table-qualified column reference: `col` or `t.col`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    pub qualifier: Option<String>,
    pub name: String,
}

impl ColumnRef {
    pub fn display(&self) -> String {
        match &self.qualifier {
            Some(q) => format!("{q}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// One entry in an aggregated select list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectItem {
    /// A passthrough column. With `GROUP BY` it must be one of the grouping
    /// columns — SQL has no meaning for a bare column alongside aggregates
    /// otherwise.
    Column { name: String, alias: Option<String> },
    /// An aggregate function applied over each group.
    Aggregate {
        call: AggregateCall,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateCall {
    pub func: AggregateFunc,
    pub arg: AggregateArg,
    /// `COUNT(DISTINCT x)` and friends deduplicate values before aggregating.
    pub distinct: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateFunc {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggregateFunc {
    pub fn name(self) -> &'static str {
        match self {
            AggregateFunc::Count => "count",
            AggregateFunc::Sum => "sum",
            AggregateFunc::Avg => "avg",
            AggregateFunc::Min => "min",
            AggregateFunc::Max => "max",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateArg {
    /// The `*` of `COUNT(*)`: every row, NULLs included.
    Star,
    /// A column; NULLs are skipped by every aggregate except `COUNT(*)`.
    Column(String),
}

/// A `HAVING` predicate. Unlike `WHERE`, its leaves are evaluated per group,
/// so they may be grouping columns or aggregate calls — including aggregates
/// that do not appear in the select list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HavingPredicate {
    Comparison {
        left: GroupExpr,
        op: ComparisonOp,
        value: Value,
    },
    And(Box<HavingPredicate>, Box<HavingPredicate>),
    Or(Box<HavingPredicate>, Box<HavingPredicate>),
}

/// Something evaluable once per output group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupExpr {
    Column(String),
    Aggregate(AggregateCall),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    Comparison {
        column: String,
        op: ComparisonOp,
        value: Value,
    },
    And(Box<Predicate>, Box<Predicate>),
    Or(Box<Predicate>, Box<Predicate>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub column: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBy {
    pub column: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    Number(i64),
    String(String),
    Comma,
    LParen,
    RParen,
    Star,
    Dot,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Semicolon,
}

pub fn parse_sql(sql: &str) -> Result<Statement> {
    let tokens = tokenize(sql)?;
    let mut parser = Parser { tokens, pos: 0 };
    let statement = parser.parse_statement()?;
    parser.consume_semicolon();

    if !parser.is_at_end() {
        return Err(DbError::Parse("unexpected tokens after statement".into()));
    }

    Ok(statement)
}

fn tokenize(sql: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = sql.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];

        match ch {
            ch if ch.is_whitespace() => index += 1,
            '-' if chars.get(index + 1) == Some(&'-') => {
                index += 2;
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '-' | '0'..='9' => {
                let start = index;
                if chars[index] == '-' {
                    index += 1;
                    if !matches!(chars.get(index), Some('0'..='9')) {
                        return Err(DbError::Parse("expected digit after '-'".into()));
                    }
                }

                while matches!(chars.get(index), Some('0'..='9')) {
                    index += 1;
                }

                let raw: String = chars[start..index].iter().collect();
                let value = raw
                    .parse::<i64>()
                    .map_err(|_| DbError::Parse(format!("invalid integer literal: {raw}")))?;
                tokens.push(Token::Number(value));
            }
            '\'' => {
                index += 1;
                let mut value = String::new();
                let mut terminated = false;

                while index < chars.len() {
                    match chars[index] {
                        '\'' if chars.get(index + 1) == Some(&'\'') => {
                            value.push('\'');
                            index += 2;
                        }
                        '\'' => {
                            index += 1;
                            terminated = true;
                            break;
                        }
                        ch => {
                            value.push(ch);
                            index += 1;
                        }
                    }
                }

                if !terminated {
                    return Err(DbError::Parse("unterminated string literal".into()));
                }

                tokens.push(Token::String(value));
            }
            '(' => {
                tokens.push(Token::LParen);
                index += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                index += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                index += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                index += 1;
            }
            '.' => {
                tokens.push(Token::Dot);
                index += 1;
            }
            '=' => {
                tokens.push(Token::Eq);
                index += 1;
            }
            '!' if chars.get(index + 1) == Some(&'=') => {
                tokens.push(Token::Ne);
                index += 2;
            }
            '<' if chars.get(index + 1) == Some(&'=') => {
                tokens.push(Token::Lte);
                index += 2;
            }
            '<' if chars.get(index + 1) == Some(&'>') => {
                tokens.push(Token::Ne);
                index += 2;
            }
            '<' => {
                tokens.push(Token::Lt);
                index += 1;
            }
            '>' if chars.get(index + 1) == Some(&'=') => {
                tokens.push(Token::Gte);
                index += 2;
            }
            '>' => {
                tokens.push(Token::Gt);
                index += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                index += 1;
            }
            ch if is_ident_start(ch) => {
                let start = index;
                index += 1;

                while matches!(chars.get(index), Some(ch) if is_ident_continue(*ch)) {
                    index += 1;
                }

                tokens.push(Token::Ident(chars[start..index].iter().collect()));
            }
            other => return Err(DbError::Parse(format!("unexpected character: {other}"))),
        }
    }

    Ok(tokens)
}

/// Decide which [`Projection`] a parsed select list represents, now that the
/// presence of a `GROUP BY`/`HAVING` is known.
///
/// The empty item list is the `SELECT *` marker. A list of plain columns with
/// no grouping stays a [`Projection::Columns`] so the simple path is
/// unchanged; a lone `COUNT(*)` with no grouping or `HAVING` stays a
/// [`Projection::CountAll`]. Everything else — any aggregate, or any grouping
/// — becomes a [`Projection::Aggregate`].
fn finalize_projection(
    items: Vec<SelectItem>,
    group_by: &[String],
    has_having: bool,
) -> Result<Projection> {
    if items.is_empty() {
        if !group_by.is_empty() {
            return Err(DbError::Parse(
                "SELECT * cannot be combined with GROUP BY".into(),
            ));
        }
        return Ok(Projection::All);
    }

    let has_aggregate = items
        .iter()
        .any(|item| matches!(item, SelectItem::Aggregate { .. }));

    if group_by.is_empty() && !has_aggregate {
        let columns = items
            .into_iter()
            .map(|item| match item {
                SelectItem::Column { name, .. } => name,
                SelectItem::Aggregate { .. } => unreachable!("no aggregates here"),
            })
            .collect();
        return Ok(Projection::Columns(columns));
    }

    // A single bare COUNT(*) with nothing else keeps the trivial fast path.
    if group_by.is_empty()
        && !has_having
        && items.len() == 1
        && let SelectItem::Aggregate { call, alias: None } = &items[0]
        && call.func == AggregateFunc::Count
        && call.arg == AggregateArg::Star
        && !call.distinct
    {
        return Ok(Projection::CountAll);
    }

    Ok(Projection::Aggregate(items))
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn parse_statement(&mut self) -> Result<Statement> {
        if self.consume_keyword("EXPLAIN") {
            let statement = self.parse_statement()?;
            return Ok(Statement::Explain(Box::new(statement)));
        }

        if self.consume_keyword("BEGIN") {
            return Ok(Statement::Begin);
        }

        if self.consume_keyword("COMMIT") {
            return Ok(Statement::Commit);
        }

        if self.consume_keyword("ROLLBACK") {
            return Ok(Statement::Rollback);
        }

        if self.consume_keyword("ANALYZE") {
            let table = if self.is_statement_boundary() {
                None
            } else {
                Some(self.expect_ident()?)
            };
            return Ok(Statement::Analyze { table });
        }

        if self.consume_keyword("CHECKPOINT") {
            return Ok(Statement::Checkpoint);
        }

        if self.consume_keyword("DROP") {
            return self.parse_drop();
        }

        if self.consume_keyword("CREATE") {
            return self.parse_create();
        }

        if self.consume_keyword("INSERT") {
            return self.parse_insert();
        }

        if self.consume_keyword("SELECT") {
            return self.parse_select();
        }

        if self.consume_keyword("UPDATE") {
            return self.parse_update();
        }

        if self.consume_keyword("DELETE") {
            return self.parse_delete();
        }

        Err(DbError::Parse("expected SQL statement".into()))
    }

    fn parse_drop(&mut self) -> Result<Statement> {
        if self.consume_keyword("TABLE") {
            return Ok(Statement::DropTable {
                name: self.expect_ident()?,
            });
        }

        if self.consume_keyword("INDEX") {
            return Ok(Statement::DropIndex {
                name: self.expect_ident()?,
            });
        }

        Err(DbError::Parse("expected TABLE or INDEX after DROP".into()))
    }

    fn parse_create(&mut self) -> Result<Statement> {
        let unique = self.consume_keyword("UNIQUE");

        if self.consume_keyword("TABLE") {
            if unique {
                return Err(DbError::Parse(
                    "CREATE UNIQUE TABLE is not supported".into(),
                ));
            }

            return self.parse_create_table();
        }

        if self.consume_keyword("INDEX") {
            return self.parse_create_index(unique);
        }

        Err(DbError::Parse(
            "expected TABLE or INDEX after CREATE".into(),
        ))
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect(Token::LParen)?;

        let mut columns = Vec::new();
        loop {
            let column_name = self.expect_ident()?;
            let data_type = self.parse_data_type()?;
            let mut column = Column::new(column_name, data_type);

            loop {
                if self.consume_keyword("PRIMARY") {
                    self.expect_keyword("KEY")?;
                    column = column.primary_key();
                } else if self.consume_keyword("UNIQUE") {
                    column = column.unique();
                } else if self.consume_keyword("NOT") {
                    self.expect_keyword("NULL")?;
                    column = column.not_null();
                } else if self.consume_keyword("NULL") {
                    column.nullable = true;
                } else {
                    break;
                }
            }

            columns.push(column);

            if !self.consume(Token::Comma) {
                break;
            }
        }

        self.expect(Token::RParen)?;
        Ok(Statement::CreateTable { name, columns })
    }

    fn parse_create_index(&mut self, unique: bool) -> Result<Statement> {
        let name = self.expect_ident()?;
        self.expect_keyword("ON")?;
        let table = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let column = self.expect_ident()?;
        self.expect(Token::RParen)?;

        Ok(Statement::CreateIndex {
            name,
            table,
            column,
            unique,
        })
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_keyword("INTO")?;
        let table = self.expect_ident()?;
        self.expect_keyword("VALUES")?;
        self.expect(Token::LParen)?;

        let mut values = Vec::new();
        loop {
            values.push(self.parse_literal()?);

            if !self.consume(Token::Comma) {
                break;
            }
        }

        self.expect(Token::RParen)?;
        Ok(Statement::Insert { table, values })
    }

    fn parse_select(&mut self) -> Result<Statement> {
        let items = self.parse_select_items()?;
        self.expect_keyword("FROM")?;
        let table = self.expect_ident()?;
        let table_alias = self.parse_optional_table_alias()?;
        let joins = self.parse_joins()?;
        let predicate = self.parse_optional_predicate()?;
        let group_by = self.parse_optional_group_by()?;
        let having = self.parse_optional_having()?;
        let order_by = self.parse_optional_order_by()?;
        let limit = self.parse_optional_limit()?;

        let projection = finalize_projection(items, &group_by, having.is_some())?;

        Ok(Statement::Select {
            table,
            table_alias,
            joins,
            projection,
            predicate,
            group_by,
            having,
            order_by,
            limit,
        })
    }

    /// `FROM t x` or `FROM t AS x`. A bare identifier is only an alias if it
    /// is not a keyword that ends the FROM item — otherwise `FROM t WHERE ...`
    /// would silently alias the table to `where`.
    fn parse_optional_table_alias(&mut self) -> Result<Option<String>> {
        if self.consume_keyword("AS") {
            return Ok(Some(self.expect_ident()?));
        }

        const RESERVED: [&str; 10] = [
            "where", "group", "having", "order", "limit", "join", "inner", "left", "cross", "on",
        ];
        if let Some(Token::Ident(word)) = self.peek() {
            let lowered = word.to_ascii_lowercase();
            if !RESERVED.contains(&lowered.as_str()) {
                return Ok(Some(self.expect_ident()?));
            }
        }
        Ok(None)
    }

    fn parse_joins(&mut self) -> Result<Vec<Join>> {
        let mut joins = Vec::new();

        loop {
            // A comma in the FROM list is an implicit CROSS JOIN.
            let kind = if self.consume(Token::Comma) {
                JoinKind::Cross
            } else if self.consume_keyword("CROSS") {
                self.expect_keyword("JOIN")?;
                JoinKind::Cross
            } else if self.consume_keyword("INNER") {
                self.expect_keyword("JOIN")?;
                JoinKind::Inner
            } else if self.consume_keyword("LEFT") {
                // OUTER is noise: LEFT JOIN and LEFT OUTER JOIN are the same.
                let _ = self.consume_keyword("OUTER");
                self.expect_keyword("JOIN")?;
                JoinKind::Left
            } else if self.consume_keyword("JOIN") {
                JoinKind::Inner // bare JOIN means INNER JOIN
            } else {
                break;
            };

            let table = self.expect_ident()?;
            let alias = self.parse_optional_table_alias()?;

            let on = if self.consume_keyword("ON") {
                Some(self.parse_join_condition()?)
            } else {
                None
            };

            if kind != JoinKind::Cross && on.is_none() {
                return Err(DbError::Parse(format!(
                    "{} JOIN requires an ON clause",
                    if kind == JoinKind::Left {
                        "LEFT"
                    } else {
                        "INNER"
                    }
                )));
            }
            if kind == JoinKind::Cross && on.is_some() {
                return Err(DbError::Parse(
                    "CROSS JOIN does not take an ON clause".into(),
                ));
            }

            joins.push(Join {
                table,
                alias,
                kind,
                on,
            });
        }

        Ok(joins)
    }

    fn parse_join_condition(&mut self) -> Result<JoinCondition> {
        let mut terms = Vec::new();
        loop {
            let left = self.parse_column_ref()?;
            let op = self.parse_comparison_op()?;
            let right = self.parse_column_ref()?;
            terms.push(JoinTerm { left, op, right });

            if !self.consume_keyword("AND") {
                break;
            }
        }
        Ok(JoinCondition { terms })
    }

    /// `col` or `qualifier.col`.
    fn parse_column_ref(&mut self) -> Result<ColumnRef> {
        let first = self.expect_ident()?;
        if self.consume(Token::Dot) {
            let name = self.expect_ident()?;
            return Ok(ColumnRef {
                qualifier: Some(first),
                name,
            });
        }
        Ok(ColumnRef {
            qualifier: None,
            name: first,
        })
    }

    /// Parse the select list into raw items. `SELECT *` is represented as an
    /// empty vector — [`finalize_projection`] turns that back into
    /// [`Projection::All`] once it knows whether a `GROUP BY` is present.
    fn parse_select_items(&mut self) -> Result<Vec<SelectItem>> {
        if self.consume(Token::Star) {
            return Ok(Vec::new()); // marker for "*"
        }

        let mut items = Vec::new();
        loop {
            if let Some(call) = self.try_parse_aggregate()? {
                let alias = self.parse_optional_alias()?;
                items.push(SelectItem::Aggregate { call, alias });
            } else {
                // Plain columns keep the current grammar: a bare identifier,
                // no alias. Aliasing is only offered on aggregates, where an
                // output name is genuinely useful.
                let name = self.expect_column_name()?;
                items.push(SelectItem::Column { name, alias: None });
            }

            if !self.consume(Token::Comma) {
                break;
            }
        }

        Ok(items)
    }

    /// If the next tokens are `FUNC(...)` for a known aggregate, consume and
    /// return the call; otherwise leave the cursor untouched and return None
    /// so the caller can treat it as a plain column.
    fn try_parse_aggregate(&mut self) -> Result<Option<AggregateCall>> {
        let func = match self.peek_aggregate_func() {
            Some(func) => func,
            None => return Ok(None),
        };

        // Only commit to the aggregate interpretation if a '(' follows, so an
        // ordinary column that happens to be named `min` still parses.
        if self.peek_at(1) != Some(&Token::LParen) {
            return Ok(None);
        }

        self.pos += 1; // function name
        self.expect(Token::LParen)?;

        let distinct = self.consume_keyword("DISTINCT");

        let arg = if self.consume(Token::Star) {
            if func != AggregateFunc::Count {
                return Err(DbError::Parse(format!(
                    "{}(*) is not allowed; only COUNT(*) is",
                    func.name().to_uppercase()
                )));
            }
            if distinct {
                return Err(DbError::Parse("COUNT(DISTINCT *) is not valid".into()));
            }
            AggregateArg::Star
        } else {
            AggregateArg::Column(self.expect_column_name()?)
        };

        self.expect(Token::RParen)?;
        Ok(Some(AggregateCall {
            func,
            arg,
            distinct,
        }))
    }

    fn peek_aggregate_func(&self) -> Option<AggregateFunc> {
        let Some(Token::Ident(word)) = self.peek() else {
            return None;
        };
        [
            AggregateFunc::Count,
            AggregateFunc::Sum,
            AggregateFunc::Avg,
            AggregateFunc::Min,
            AggregateFunc::Max,
        ]
        .into_iter()
        .find(|func| word.eq_ignore_ascii_case(func.name()))
    }

    fn parse_optional_alias(&mut self) -> Result<Option<String>> {
        if self.consume_keyword("AS") {
            return Ok(Some(self.expect_ident()?));
        }
        Ok(None)
    }

    fn parse_optional_group_by(&mut self) -> Result<Vec<String>> {
        if !self.consume_keyword("GROUP") {
            return Ok(Vec::new());
        }
        self.expect_keyword("BY")?;

        let mut columns = Vec::new();
        loop {
            columns.push(self.expect_column_name()?);
            if !self.consume(Token::Comma) {
                break;
            }
        }
        Ok(columns)
    }

    fn parse_optional_having(&mut self) -> Result<Option<HavingPredicate>> {
        if !self.consume_keyword("HAVING") {
            return Ok(None);
        }
        self.parse_having_or().map(Some)
    }

    fn parse_having_or(&mut self) -> Result<HavingPredicate> {
        let mut left = self.parse_having_and()?;
        while self.consume_keyword("OR") {
            let right = self.parse_having_and()?;
            left = HavingPredicate::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_having_and(&mut self) -> Result<HavingPredicate> {
        let mut left = self.parse_having_primary()?;
        while self.consume_keyword("AND") {
            let right = self.parse_having_primary()?;
            left = HavingPredicate::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_having_primary(&mut self) -> Result<HavingPredicate> {
        if self.consume(Token::LParen) {
            let inner = self.parse_having_or()?;
            self.expect(Token::RParen)?;
            return Ok(inner);
        }

        let left = if let Some(call) = self.try_parse_aggregate()? {
            GroupExpr::Aggregate(call)
        } else {
            GroupExpr::Column(self.expect_column_name()?)
        };
        let op = self.parse_comparison_op()?;
        let value = self.parse_literal()?;
        Ok(HavingPredicate::Comparison { left, op, value })
    }

    fn parse_update(&mut self) -> Result<Statement> {
        let table = self.expect_ident()?;
        self.expect_keyword("SET")?;

        let mut assignments = Vec::new();
        loop {
            let column = self.expect_ident()?;
            self.expect(Token::Eq)?;
            let value = self.parse_literal()?;
            assignments.push(Assignment { column, value });

            if !self.consume(Token::Comma) {
                break;
            }
        }

        let predicate = self.parse_optional_predicate()?;

        Ok(Statement::Update {
            table,
            assignments,
            predicate,
        })
    }

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect_keyword("FROM")?;
        let table = self.expect_ident()?;
        let predicate = self.parse_optional_predicate()?;
        Ok(Statement::Delete { table, predicate })
    }

    fn parse_optional_predicate(&mut self) -> Result<Option<Predicate>> {
        if !self.consume_keyword("WHERE") {
            return Ok(None);
        }

        self.parse_or().map(Some)
    }

    fn parse_or(&mut self) -> Result<Predicate> {
        let mut left = self.parse_and()?;

        while self.consume_keyword("OR") {
            let right = self.parse_and()?;
            left = Predicate::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Predicate> {
        let mut left = self.parse_predicate_primary()?;

        while self.consume_keyword("AND") {
            let right = self.parse_predicate_primary()?;
            left = Predicate::And(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_predicate_primary(&mut self) -> Result<Predicate> {
        if self.consume(Token::LParen) {
            let predicate = self.parse_or()?;
            self.expect(Token::RParen)?;
            return Ok(predicate);
        }

        let column = self.expect_column_name()?;
        let op = self.parse_comparison_op()?;
        let value = self.parse_literal()?;
        Ok(Predicate::Comparison { column, op, value })
    }

    fn parse_comparison_op(&mut self) -> Result<ComparisonOp> {
        match self.advance() {
            Some(Token::Eq) => Ok(ComparisonOp::Eq),
            Some(Token::Ne) => Ok(ComparisonOp::Ne),
            Some(Token::Lt) => Ok(ComparisonOp::Lt),
            Some(Token::Lte) => Ok(ComparisonOp::Lte),
            Some(Token::Gt) => Ok(ComparisonOp::Gt),
            Some(Token::Gte) => Ok(ComparisonOp::Gte),
            Some(other) => Err(DbError::Parse(format!(
                "expected comparison operator, got {other:?}"
            ))),
            None => Err(DbError::Parse(
                "expected comparison operator, got end of input".into(),
            )),
        }
    }

    fn parse_optional_order_by(&mut self) -> Result<Option<OrderBy>> {
        if !self.consume_keyword("ORDER") {
            return Ok(None);
        }

        self.expect_keyword("BY")?;
        let column = self.expect_column_name()?;
        let direction = if self.consume_keyword("DESC") {
            SortDirection::Desc
        } else {
            let _ = self.consume_keyword("ASC");
            SortDirection::Asc
        };

        Ok(Some(OrderBy { column, direction }))
    }

    fn parse_optional_limit(&mut self) -> Result<Option<usize>> {
        if !self.consume_keyword("LIMIT") {
            return Ok(None);
        }

        let limit = match self.advance() {
            Some(Token::Number(value)) if value >= 0 => value as usize,
            Some(other) => {
                return Err(DbError::Parse(format!(
                    "expected non-negative LIMIT literal, got {other:?}"
                )));
            }
            None => return Err(DbError::Parse("expected LIMIT literal".into())),
        };

        Ok(Some(limit))
    }

    fn parse_data_type(&mut self) -> Result<DataType> {
        let ident = self.expect_ident()?;

        match ident.as_str() {
            "int" | "integer" => Ok(DataType::Int),
            "text" => Ok(DataType::Text),
            "bool" | "boolean" => Ok(DataType::Bool),
            other => Err(DbError::Parse(format!("unknown data type: {other}"))),
        }
    }

    fn parse_literal(&mut self) -> Result<Value> {
        match self.advance() {
            Some(Token::Number(value)) => Ok(Value::Int(value)),
            Some(Token::String(value)) => Ok(Value::Text(value)),
            Some(Token::Ident(value)) if value.eq_ignore_ascii_case("true") => {
                Ok(Value::Bool(true))
            }
            Some(Token::Ident(value)) if value.eq_ignore_ascii_case("false") => {
                Ok(Value::Bool(false))
            }
            Some(Token::Ident(value)) if value.eq_ignore_ascii_case("null") => Ok(Value::Null),
            Some(other) => Err(DbError::Parse(format!("expected literal, got {other:?}"))),
            None => Err(DbError::Parse("expected literal, got end of input".into())),
        }
    }

    /// A column name, possibly table-qualified. `t.col` becomes the single
    /// identifier "t.col"; the executor resolves it against the (combined)
    /// schema. Keeping it as one string means projections, predicates,
    /// ORDER BY and GROUP BY need no AST changes to support joins.
    fn expect_column_name(&mut self) -> Result<String> {
        let first = self.expect_ident()?;
        if self.consume(Token::Dot) {
            let second = self.expect_ident()?;
            return Ok(format!("{first}.{second}"));
        }
        Ok(first)
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Some(Token::Ident(value)) => Ok(normalize_identifier(&value)),
            Some(other) => Err(DbError::Parse(format!(
                "expected identifier, got {other:?}"
            ))),
            None => Err(DbError::Parse(
                "expected identifier, got end of input".into(),
            )),
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<()> {
        if self.consume_keyword(keyword) {
            return Ok(());
        }

        Err(DbError::Parse(format!("expected keyword {keyword}")))
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        match self.peek() {
            Some(Token::Ident(value)) if value.eq_ignore_ascii_case(keyword) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn expect(&mut self, token: Token) -> Result<()> {
        if self.consume(token.clone()) {
            Ok(())
        } else {
            Err(DbError::Parse(format!("expected token {token:?}")))
        }
    }

    fn consume(&mut self, token: Token) -> bool {
        if self.peek() == Some(&token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_semicolon(&mut self) {
        let _ = self.consume(Token::Semicolon);
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += usize::from(token.is_some());
        token
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
    }

    fn is_statement_boundary(&self) -> bool {
        matches!(self.peek(), None | Some(Token::Semicolon))
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }
}
