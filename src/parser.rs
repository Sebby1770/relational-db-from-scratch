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
        table: String,
        projection: Projection,
        predicate: Option<Predicate>,
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
    CountAll,
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
        let projection = self.parse_projection()?;
        self.expect_keyword("FROM")?;
        let table = self.expect_ident()?;
        let predicate = self.parse_optional_predicate()?;
        let order_by = self.parse_optional_order_by()?;
        let limit = self.parse_optional_limit()?;

        Ok(Statement::Select {
            table,
            projection,
            predicate,
            order_by,
            limit,
        })
    }

    fn parse_projection(&mut self) -> Result<Projection> {
        if self.consume(Token::Star) {
            return Ok(Projection::All);
        }

        if self.consume_keyword("COUNT") {
            self.expect(Token::LParen)?;
            self.expect(Token::Star)?;
            self.expect(Token::RParen)?;
            return Ok(Projection::CountAll);
        }

        let mut columns = Vec::new();
        loop {
            columns.push(self.expect_ident()?);

            if !self.consume(Token::Comma) {
                break;
            }
        }

        Ok(Projection::Columns(columns))
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

        let column = self.expect_ident()?;
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
        let column = self.expect_ident()?;
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

    fn is_statement_boundary(&self) -> bool {
        matches!(self.peek(), None | Some(Token::Semicolon))
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }
}
