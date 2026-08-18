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
        source: InsertSource,
    },
    Select(Box<SelectStatement>),
    Update {
        table: String,
        assignments: Vec<Assignment>,
        predicate: Option<Predicate>,
    },
    Delete {
        table: String,
        predicate: Option<Predicate>,
    },
    CopyFrom {
        table: String,
        path: String,
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
    CountAll,
    Items(Vec<SelectItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertSource {
    Values(Vec<Value>),
    Select(Box<SelectStatement>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectStatement {
    pub distinct: bool,
    pub table: String,
    pub alias: Option<String>,
    pub joins: Vec<JoinClause>,
    pub projection: Projection,
    pub predicate: Option<Predicate>,
    pub group_by: Vec<String>,
    pub having: Option<Predicate>,
    pub order_by: Vec<OrderBy>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectItem {
    Column(String),
    CountAll,
    Sum(String),
    Min(String),
    Max(String),
    Avg(String),
}

impl SelectItem {
    pub fn is_aggregate(&self) -> bool {
        !matches!(self, SelectItem::Column(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinClause {
    pub join_type: JoinType,
    pub table: String,
    pub alias: Option<String>,
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    Comparison {
        expr: SelectItem,
        op: ComparisonOp,
        value: Value,
    },
    Between {
        expr: SelectItem,
        low: Value,
        high: Value,
    },
    InList {
        expr: SelectItem,
        values: Vec<Value>,
    },
    Like {
        column: String,
        pattern: String,
        escape: Option<char>,
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

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_reserved_after_table(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "WHERE"
            | "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "FULL"
            | "CROSS"
            | "OUTER"
            | "ON"
            | "GROUP"
            | "ORDER"
            | "LIMIT"
            | "OFFSET"
            | "HAVING"
            | "UNION"
            | "EXCEPT"
            | "INTERSECT"
            | "AS"
            | "DISTINCT"
            | "BETWEEN"
            | "IN"
    )
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

        if self.consume_keyword("COPY") {
            return self.parse_copy();
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

        if self.consume_keyword("SELECT") {
            return Ok(Statement::Insert {
                table,
                source: InsertSource::Select(Box::new(self.parse_select_statement()?)),
            });
        }

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
        Ok(Statement::Insert {
            table,
            source: InsertSource::Values(values),
        })
    }

    fn parse_copy(&mut self) -> Result<Statement> {
        let table = self.expect_ident()?;
        self.expect_keyword("FROM")?;
        let path = match self.advance() {
            Some(Token::String(path)) => path,
            Some(other) => {
                return Err(DbError::Parse(format!(
                    "expected csv path string, got {other:?}"
                )));
            }
            None => return Err(DbError::Parse("expected csv path string".into())),
        };

        Ok(Statement::CopyFrom { table, path })
    }

    fn parse_select(&mut self) -> Result<Statement> {
        Ok(Statement::Select(Box::new(self.parse_select_statement()?)))
    }

    fn parse_select_statement(&mut self) -> Result<SelectStatement> {
        let distinct = self.consume_keyword("DISTINCT");
        let projection = self.parse_projection()?;
        self.expect_keyword("FROM")?;
        let (table, alias) = self.parse_table_ref()?;
        let joins = self.parse_joins()?;
        let predicate = self.parse_optional_predicate()?;
        let group_by = self.parse_optional_group_by()?;
        let having = self.parse_optional_having()?;
        let order_by = self.parse_order_by_list()?;
        let (limit, offset) = self.parse_optional_limit_offset()?;

        Ok(SelectStatement {
            distinct,
            table,
            alias,
            joins,
            projection,
            predicate,
            group_by,
            having,
            order_by,
            limit,
            offset,
        })
    }

    fn parse_projection(&mut self) -> Result<Projection> {
        if self.consume(Token::Star) {
            return Ok(Projection::All);
        }

        let mut items = Vec::new();
        loop {
            items.push(self.parse_select_item()?);
            if !self.consume(Token::Comma) {
                break;
            }
        }

        if items.len() == 1 && matches!(items[0], SelectItem::CountAll) {
            return Ok(Projection::CountAll);
        }

        if items
            .iter()
            .all(|item| matches!(item, SelectItem::Column(_)))
        {
            let columns = items
                .into_iter()
                .map(|item| match item {
                    SelectItem::Column(column) => column,
                    _ => unreachable!("filtered to columns"),
                })
                .collect();
            return Ok(Projection::Columns(columns));
        }

        Ok(Projection::Items(items))
    }

    fn parse_select_item(&mut self) -> Result<SelectItem> {
        if self.peek_function("COUNT") {
            self.advance();
            self.expect(Token::LParen)?;
            self.expect(Token::Star)?;
            self.expect(Token::RParen)?;
            return Ok(SelectItem::CountAll);
        }

        if self.peek_function("SUM") {
            self.advance();
            self.expect(Token::LParen)?;
            let column = self.parse_column_ref()?;
            self.expect(Token::RParen)?;
            return Ok(SelectItem::Sum(column));
        }

        if self.peek_function("MIN") {
            self.advance();
            self.expect(Token::LParen)?;
            let column = self.parse_column_ref()?;
            self.expect(Token::RParen)?;
            return Ok(SelectItem::Min(column));
        }

        if self.peek_function("MAX") {
            self.advance();
            self.expect(Token::LParen)?;
            let column = self.parse_column_ref()?;
            self.expect(Token::RParen)?;
            return Ok(SelectItem::Max(column));
        }

        if self.peek_function("AVG") {
            self.advance();
            self.expect(Token::LParen)?;
            let column = self.parse_column_ref()?;
            self.expect(Token::RParen)?;
            return Ok(SelectItem::Avg(column));
        }

        Ok(SelectItem::Column(self.parse_column_ref()?))
    }

    fn parse_table_ref(&mut self) -> Result<(String, Option<String>)> {
        let table = self.expect_ident()?;
        if self.consume_keyword("AS") {
            return Ok((table, Some(self.expect_ident()?)));
        }
        if self.next_is_table_alias() {
            return Ok((table, Some(self.expect_ident()?)));
        }
        Ok((table, None))
    }

    fn next_is_table_alias(&self) -> bool {
        match self.peek() {
            Some(Token::Ident(value)) => !is_reserved_after_table(value),
            _ => false,
        }
    }

    fn parse_joins(&mut self) -> Result<Vec<JoinClause>> {
        let mut joins = Vec::new();

        loop {
            let join_type = if self.consume_keyword("INNER") {
                self.expect_keyword("JOIN")?;
                JoinType::Inner
            } else if self.consume_keyword("LEFT") {
                let _ = self.consume_keyword("OUTER");
                self.expect_keyword("JOIN")?;
                JoinType::Left
            } else if self.consume_keyword("JOIN") {
                JoinType::Inner
            } else {
                break;
            };

            let (table, alias) = self.parse_table_ref()?;
            self.expect_keyword("ON")?;
            let left = self.parse_column_ref()?;
            self.expect(Token::Eq)?;
            let right = self.parse_column_ref()?;
            joins.push(JoinClause {
                join_type,
                table,
                alias,
                left,
                right,
            });
        }

        Ok(joins)
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

        let expr = self.parse_select_item()?;
        if self.consume_keyword("BETWEEN") {
            let low = self.parse_literal()?;
            self.expect_keyword("AND")?;
            let high = self.parse_literal()?;
            return Ok(Predicate::Between { expr, low, high });
        }

        if self.consume_keyword("IN") {
            return self.parse_in_list(expr);
        }

        if self.consume_keyword("LIKE") {
            let SelectItem::Column(column) = expr else {
                return Err(DbError::Parse(
                    "LIKE requires a column on the left-hand side".into(),
                ));
            };
            return self.parse_like(column);
        }

        let op = self.parse_comparison_op()?;
        let value = self.parse_literal()?;
        Ok(Predicate::Comparison { expr, op, value })
    }

    fn parse_in_list(&mut self, expr: SelectItem) -> Result<Predicate> {
        self.expect(Token::LParen)?;
        let mut values = Vec::new();
        loop {
            values.push(self.parse_literal()?);
            if !self.consume(Token::Comma) {
                break;
            }
        }
        self.expect(Token::RParen)?;
        if values.is_empty() {
            return Err(DbError::Parse("expected value in IN list".into()));
        }
        Ok(Predicate::InList { expr, values })
    }

    fn parse_like(&mut self, column: String) -> Result<Predicate> {
        let pattern = match self.advance() {
            Some(Token::String(pattern)) => pattern,
            Some(other) => {
                return Err(DbError::Parse(format!(
                    "expected LIKE pattern string, got {other:?}"
                )));
            }
            None => return Err(DbError::Parse("expected LIKE pattern string".into())),
        };

        let escape = if self.consume_keyword("ESCAPE") {
            match self.advance() {
                Some(Token::String(value)) => {
                    let mut chars = value.chars();
                    match (chars.next(), chars.next()) {
                        (Some(escape), None) => Some(escape),
                        _ => {
                            return Err(DbError::Parse(
                                "ESCAPE clause must be a single character".into(),
                            ));
                        }
                    }
                }
                Some(other) => {
                    return Err(DbError::Parse(format!(
                        "expected ESCAPE character string, got {other:?}"
                    )));
                }
                None => return Err(DbError::Parse("expected ESCAPE character string".into())),
            }
        } else {
            None
        };

        Ok(Predicate::Like {
            column,
            pattern,
            escape,
        })
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

    fn parse_optional_group_by(&mut self) -> Result<Vec<String>> {
        if !self.consume_keyword("GROUP") {
            return Ok(Vec::new());
        }

        self.expect_keyword("BY")?;
        let mut columns = Vec::new();
        loop {
            columns.push(self.parse_column_ref()?);
            if !self.consume(Token::Comma) {
                break;
            }
        }

        if columns.is_empty() {
            return Err(DbError::Parse("expected column after GROUP BY".into()));
        }

        Ok(columns)
    }

    fn parse_optional_having(&mut self) -> Result<Option<Predicate>> {
        if !self.consume_keyword("HAVING") {
            return Ok(None);
        }

        self.parse_or().map(Some)
    }

    fn parse_order_by_list(&mut self) -> Result<Vec<OrderBy>> {
        if !self.consume_keyword("ORDER") {
            return Ok(Vec::new());
        }

        self.expect_keyword("BY")?;
        let mut keys = Vec::new();
        loop {
            let column = self.parse_column_ref()?;
            let direction = if self.consume_keyword("DESC") {
                SortDirection::Desc
            } else {
                let _ = self.consume_keyword("ASC");
                SortDirection::Asc
            };
            keys.push(OrderBy { column, direction });
            if !self.consume(Token::Comma) {
                break;
            }
        }

        if keys.is_empty() {
            return Err(DbError::Parse("expected column after ORDER BY".into()));
        }

        Ok(keys)
    }

    fn parse_optional_limit_offset(&mut self) -> Result<(Option<usize>, Option<usize>)> {
        if self.consume_keyword("OFFSET") {
            let offset = self.expect_non_negative_int("OFFSET")?;
            return Ok((None, Some(offset)));
        }

        if !self.consume_keyword("LIMIT") {
            return Ok((None, None));
        }

        let limit = self.expect_non_negative_int("LIMIT")?;
        let offset = if self.consume_keyword("OFFSET") {
            Some(self.expect_non_negative_int("OFFSET")?)
        } else {
            None
        };

        Ok((Some(limit), offset))
    }

    fn expect_non_negative_int(&mut self, label: &str) -> Result<usize> {
        match self.advance() {
            Some(Token::Number(value)) if value >= 0 => Ok(value as usize),
            Some(other) => Err(DbError::Parse(format!(
                "expected non-negative {label} literal, got {other:?}"
            ))),
            None => Err(DbError::Parse(format!("expected {label} literal"))),
        }
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

    fn parse_column_ref(&mut self) -> Result<String> {
        let first = self.expect_ident()?;
        if self.consume(Token::Dot) {
            let second = self.expect_ident()?;
            Ok(format!("{first}.{second}"))
        } else {
            Ok(first)
        }
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

    fn peek_function(&self, name: &str) -> bool {
        match (self.peek(), self.tokens.get(self.pos + 1)) {
            (Some(Token::Ident(ident)), Some(Token::LParen)) => ident.eq_ignore_ascii_case(name),
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

    fn is_statement_boundary(&self) -> bool {
        matches!(self.peek(), None | Some(Token::Semicolon))
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }
}
