use jarvix_common::error::{JarvixError, Result};
use crate::token::Token;

pub struct Lexer<'a> {
    input: &'a str,
    chars: Vec<char>,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();

        while let Some(ch) = self.peek() {
            match ch {
                // Whitespace
                ' ' | '\t' | '\r' | '\n' => {
                    self.advance();
                }
                // Comments (-- ...)
                '-' if self.peek_next() == Some('-') => {
                    self.advance();
                    self.advance();
                    while let Some(c) = self.peek() {
                        self.advance();
                        if c == '\n' {
                            break;
                        }
                    }
                }
                // Operators & Symbols
                '=' => {
                    self.advance();
                    tokens.push(Token::Equal);
                }
                '!' if self.peek_next() == Some('=') => {
                    self.advance();
                    self.advance();
                    tokens.push(Token::NotEqual);
                }
                '<' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::LessThanOrEqual);
                    } else if self.peek() == Some('>') {
                        self.advance();
                        tokens.push(Token::NotEqual);
                    } else {
                        tokens.push(Token::LessThan);
                    }
                }
                '>' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::GreaterThanOrEqual);
                    } else {
                        tokens.push(Token::GreaterThan);
                    }
                }
                '+' => {
                    self.advance();
                    tokens.push(Token::Plus);
                }
                '-' => {
                    self.advance();
                    tokens.push(Token::Minus);
                }
                '*' => {
                    self.advance();
                    tokens.push(Token::Star);
                }
                '/' => {
                    self.advance();
                    tokens.push(Token::Slash);
                }
                ',' => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                ';' => {
                    self.advance();
                    tokens.push(Token::Semicolon);
                }
                '(' => {
                    self.advance();
                    tokens.push(Token::LParen);
                }
                ')' => {
                    self.advance();
                    tokens.push(Token::RParen);
                }
                // String literal '...'
                '\'' => {
                    tokens.push(self.read_string()?);
                }
                // Numbers
                '0'..='9' => {
                    tokens.push(self.read_number()?);
                }
                // Identifiers or Keywords
                'a'..='z' | 'A'..='Z' | '_' => {
                    tokens.push(self.read_ident_or_keyword());
                }
                _ => {
                    return Err(JarvixError::Syntax(format!(
                        "Unexpected character '{}' in SQL input",
                        ch
                    )));
                }
            }
        }

        tokens.push(Token::Eof);
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        Some(ch)
    }

    fn read_string(&mut self) -> Result<Token> {
        self.advance(); // consume opening quote
        let mut s = String::new();

        while let Some(ch) = self.advance() {
            if ch == '\'' {
                if self.peek() == Some('\'') {
                    // Escaped quote ''
                    self.advance();
                    s.push('\'');
                } else {
                    return Ok(Token::StringLit(s));
                }
            } else {
                s.push(ch);
            }
        }

        Err(JarvixError::Syntax("Unterminated string literal".to_string()))
    }

    fn read_number(&mut self) -> Result<Token> {
        let mut num_str = String::new();
        let mut is_float = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else if ch == '.' && !is_float && self.peek_next().map_or(false, |c| c.is_ascii_digit()) {
                is_float = true;
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if is_float {
            let val = num_str.parse::<f64>().map_err(|e| JarvixError::Syntax(e.to_string()))?;
            Ok(Token::FloatLit(val))
        } else {
            let val = num_str.parse::<i64>().map_err(|e| JarvixError::Syntax(e.to_string()))?;
            Ok(Token::IntegerLit(val))
        }
    }

    fn read_ident_or_keyword(&mut self) -> Token {
        let mut word = String::new();

        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                word.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let upper = word.to_uppercase();
        match upper.as_str() {
            "SELECT" => Token::Select,
            "FROM" => Token::From,
            "WHERE" => Token::Where,
            "INSERT" => Token::Insert,
            "INTO" => Token::Into,
            "VALUES" => Token::Values,
            "UPDATE" => Token::Update,
            "SET" => Token::Set,
            "DELETE" => Token::Delete,
            "CREATE" => Token::Create,
            "TABLE" => Token::Table,
            "DROP" => Token::Drop,
            "INDEX" => Token::Index,
            "ON" => Token::On,
            "UNIQUE" => Token::Unique,
            "IF" => Token::If,
            "EXISTS" => Token::Exists,
            "AND" => Token::And,
            "OR" => Token::Or,
            "NOT" => Token::Not,
            "AS" => Token::As,
            "LIMIT" => Token::Limit,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "NULL" => Token::Null,
            "TRUE" => Token::BooleanLit(true),
            "FALSE" => Token::BooleanLit(false),
            "INT" => Token::Int,
            "INTEGER" => Token::Integer,
            "BIGINT" => Token::BigInt,
            "SMALLINT" => Token::SmallInt,
            "BOOLEAN" => Token::Boolean,
            "BOOL" => Token::Bool,
            "VARCHAR" => Token::Varchar,
            "TEXT" => Token::Text,
            "FLOAT" => Token::Float,
            "DOUBLE" => Token::Double,
            _ => Token::Ident(word),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_sql_tokens() {
        let sql = "SELECT id, name FROM users WHERE age >= 18 AND active = true;";
        let mut lexer = Lexer::new(sql);
        let tokens = lexer.tokenize().unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Select,
                Token::Ident("id".to_string()),
                Token::Comma,
                Token::Ident("name".to_string()),
                Token::From,
                Token::Ident("users".to_string()),
                Token::Where,
                Token::Ident("age".to_string()),
                Token::GreaterThanOrEqual,
                Token::IntegerLit(18),
                Token::And,
                Token::Ident("active".to_string()),
                Token::Equal,
                Token::BooleanLit(true),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }
}
