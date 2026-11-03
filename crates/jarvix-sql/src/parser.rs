use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{DataType, Value};

use crate::ast::*;
use crate::lexer::Lexer;
use crate::token::Token;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_sql(sql: &str) -> Result<Statement> {
        let mut lexer = Lexer::new(sql);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser::new(tokens);
        parser.parse_statement()
    }

    pub fn parse_statement(&mut self) -> Result<Statement> {
        let stmt = match self.peek() {
            Token::Create => self.parse_create()?,
            Token::Drop => self.parse_drop()?,
            Token::Select => self.parse_select()?,
            Token::Insert => self.parse_insert()?,
            Token::Update => self.parse_update()?,
            Token::Delete => self.parse_delete()?,
            tok => {
                return Err(JarvixError::Syntax(format!(
                    "Expected SQL statement keyword, found {:?}",
                    tok
                )));
            }
        };

        // Consume optional trailing semicolon
        if self.peek() == Token::Semicolon {
            self.advance();
        }

        Ok(stmt)
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: Token) -> Result<()> {
        let actual = self.advance();
        if actual == expected {
            Ok(())
        } else {
            Err(JarvixError::Syntax(format!(
                "Expected token {:?}, found {:?}",
                expected, actual
            )))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            tok => Err(JarvixError::Syntax(format!(
                "Expected identifier, found {:?}",
                tok
            ))),
        }
    }

    // --- CREATE statements ---
    fn parse_create(&mut self) -> Result<Statement> {
        self.expect(Token::Create)?;

        if self.peek() == Token::Unique || self.peek() == Token::Index {
            let unique = if self.peek() == Token::Unique {
                self.advance();
                true
            } else {
                false
            };
            self.expect(Token::Index)?;
            let index_name = self.expect_ident()?;
            self.expect(Token::On)?;
            let table = self.expect_ident()?;
            self.expect(Token::LParen)?;
            let column = self.expect_ident()?;
            self.expect(Token::RParen)?;
            return Ok(Statement::CreateIndex {
                name: index_name,
                table,
                column,
                unique,
            });
        }

        self.expect(Token::Table)?;
        let mut if_not_exists = false;
        if self.peek() == Token::If {
            self.advance();
            self.expect(Token::Not)?;
            self.expect(Token::Exists)?;
            if_not_exists = true;
        }

        let table_name = self.expect_ident()?;
        self.expect(Token::LParen)?;

        let mut columns = Vec::new();
        while self.peek() != Token::RParen && self.peek() != Token::Eof {
            let col_name = self.expect_ident()?;
            let data_type = self.parse_data_type()?;
            let mut nullable = true;
            let primary_key = false;

            while self.peek() != Token::Comma && self.peek() != Token::RParen && self.peek() != Token::Eof {
                if self.peek() == Token::Not {
                    self.advance();
                    self.expect(Token::Null)?;
                    nullable = false;
                } else {
                    self.advance(); // skip other constraints for now
                }
            }

            columns.push(ColumnDef {
                name: col_name,
                data_type,
                nullable,
                primary_key,
            });

            if self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::RParen)?;
        Ok(Statement::CreateTable {
            name: table_name,
            columns,
            if_not_exists,
        })
    }

    fn parse_data_type(&mut self) -> Result<DataType> {
        match self.advance() {
            Token::Int | Token::Integer => Ok(DataType::Integer),
            Token::BigInt => Ok(DataType::BigInt),
            Token::SmallInt => Ok(DataType::SmallInt),
            Token::Boolean | Token::Bool => Ok(DataType::Boolean),
            Token::Text => Ok(DataType::Text),
            Token::Float => Ok(DataType::Float),
            Token::Double => Ok(DataType::Double),
            Token::Varchar => {
                let mut len = 255;
                if self.peek() == Token::LParen {
                    self.advance();
                    if let Token::IntegerLit(n) = self.advance() {
                        len = n as u32;
                    }
                    self.expect(Token::RParen)?;
                }
                Ok(DataType::Varchar(len))
            }
            tok => Err(JarvixError::Syntax(format!(
                "Expected data type, found {:?}",
                tok
            ))),
        }
    }

    // --- DROP statements ---
    fn parse_drop(&mut self) -> Result<Statement> {
        self.expect(Token::Drop)?;
        self.expect(Token::Table)?;

        let mut if_exists = false;
        if self.peek() == Token::If {
            self.advance();
            self.expect(Token::Exists)?;
            if_exists = true;
        }

        let name = self.expect_ident()?;
        Ok(Statement::DropTable { name, if_exists })
    }

    // --- SELECT statements ---
    fn parse_select(&mut self) -> Result<Statement> {
        self.expect(Token::Select)?;

        let mut projections = Vec::new();
        loop {
            if self.peek() == Token::Star {
                self.advance();
                projections.push(SelectItem::Wildcard);
            } else {
                let expr = self.parse_expr()?;
                let mut alias = None;
                if self.peek() == Token::As {
                    self.advance();
                    alias = Some(self.expect_ident()?);
                } else if let Token::Ident(a) = self.peek() {
                    self.advance();
                    alias = Some(a);
                }
                projections.push(SelectItem::Expr { expr, alias });
            }

            if self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::From)?;
        let from_table = self.expect_ident()?;

        let mut filter = None;
        if self.peek() == Token::Where {
            self.advance();
            filter = Some(self.parse_expr()?);
        }

        let mut limit = None;
        if self.peek() == Token::Limit {
            self.advance();
            if let Token::IntegerLit(n) = self.advance() {
                limit = Some(n as usize);
            }
        }

        Ok(Statement::Select {
            projections,
            from: from_table,
            filter,
            limit,
        })
    }

    // --- INSERT statements ---
    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect(Token::Insert)?;
        self.expect(Token::Into)?;
        let table = self.expect_ident()?;

        let mut columns = None;
        if self.peek() == Token::LParen {
            self.advance();
            let mut cols = Vec::new();
            while self.peek() != Token::RParen && self.peek() != Token::Eof {
                cols.push(self.expect_ident()?);
                if self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(Token::RParen)?;
            columns = Some(cols);
        }

        self.expect(Token::Values)?;

        let mut values = Vec::new();
        loop {
            self.expect(Token::LParen)?;
            let mut row = Vec::new();
            while self.peek() != Token::RParen && self.peek() != Token::Eof {
                row.push(self.parse_expr()?);
                if self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(Token::RParen)?;
            values.push(row);

            if self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    // --- UPDATE statements ---
    fn parse_update(&mut self) -> Result<Statement> {
        self.expect(Token::Update)?;
        let table = self.expect_ident()?;
        self.expect(Token::Set)?;

        let mut assignments = Vec::new();
        loop {
            let col = self.expect_ident()?;
            self.expect(Token::Equal)?;
            let val = self.parse_expr()?;
            assignments.push((col, val));

            if self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        let mut filter = None;
        if self.peek() == Token::Where {
            self.advance();
            filter = Some(self.parse_expr()?);
        }

        Ok(Statement::Update {
            table,
            assignments,
            filter,
        })
    }

    // --- DELETE statements ---
    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect(Token::Delete)?;
        self.expect(Token::From)?;
        let table = self.expect_ident()?;

        let mut filter = None;
        if self.peek() == Token::Where {
            self.advance();
            filter = Some(self.parse_expr()?);
        }

        Ok(Statement::Delete { table, filter })
    }

    // --- Expression Parsing with Precedence ---
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.peek() == Token::Or {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOperator::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;
        while self.peek() == Token::And {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOperator::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::Equal => BinaryOperator::Eq,
                Token::NotEqual => BinaryOperator::NotEq,
                Token::LessThan => BinaryOperator::Lt,
                Token::LessThanOrEqual => BinaryOperator::LtEq,
                Token::GreaterThan => BinaryOperator::Gt,
                Token::GreaterThanOrEqual => BinaryOperator::GtEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOperator::Plus,
                Token::Minus => BinaryOperator::Minus,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOperator::Multiply,
                Token::Slash => BinaryOperator::Divide,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek() == Token::Not {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOperator::Not,
                expr: Box::new(expr),
            });
        }
        if self.peek() == Token::Minus {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOperator::Neg,
                expr: Box::new(expr),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.advance() {
            Token::IntegerLit(n) => Ok(Expr::Literal(Value::Integer(n as i32))),
            Token::FloatLit(f) => Ok(Expr::Literal(Value::Double(f))),
            Token::StringLit(s) => Ok(Expr::Literal(Value::Text(s))),
            Token::BooleanLit(b) => Ok(Expr::Literal(Value::Boolean(b))),
            Token::Null => Ok(Expr::Literal(Value::Null)),
            Token::Ident(name) => Ok(Expr::Column(name)),
            Token::LParen => {
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            tok => Err(JarvixError::Syntax(format!(
                "Unexpected token in expression: {:?}",
                tok
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_create_table() {
        let sql = "CREATE TABLE users (id INT NOT NULL, name VARCHAR(100), active BOOL);";
        let stmt = Parser::parse_sql(sql).unwrap();

        match stmt {
            Statement::CreateTable { name, columns, .. } => {
                assert_eq!(name, "users");
                assert_eq!(columns.len(), 3);
                assert_eq!(columns[0].name, "id");
                assert_eq!(columns[0].data_type, DataType::Integer);
                assert!(!columns[0].nullable);
            }
            _ => panic!("Expected CreateTable"),
        }
    }

    #[test]
    fn test_parse_select_query() {
        let sql = "SELECT id, name AS username FROM users WHERE age >= 21 AND active = true LIMIT 10;";
        let stmt = Parser::parse_sql(sql).unwrap();

        match stmt {
            Statement::Select { projections, from, filter, limit } => {
                assert_eq!(from, "users");
                assert_eq!(projections.len(), 2);
                assert!(filter.is_some());
                assert_eq!(limit, Some(10));
            }
            _ => panic!("Expected Select"),
        }
    }

    #[test]
    fn test_parse_insert_statement() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice'), (2, 'Bob');";
        let stmt = Parser::parse_sql(sql).unwrap();

        match stmt {
            Statement::Insert { table, columns, values } => {
                assert_eq!(table, "users");
                assert_eq!(columns.unwrap(), vec!["id", "name"]);
                assert_eq!(values.len(), 2);
            }
            _ => panic!("Expected Insert"),
        }
    }
}
