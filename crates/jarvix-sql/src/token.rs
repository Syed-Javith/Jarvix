#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Select,
    From,
    Where,
    Insert,
    Into,
    Values,
    Update,
    Set,
    Delete,
    Create,
    Table,
    Drop,
    Index,
    On,
    Unique,
    If,
    Exists,
    And,
    Or,
    Not,
    As,
    Limit,
    Order,
    By,
    Asc,
    Desc,
    Null,

    // Data type keywords
    Int,
    Integer,
    BigInt,
    SmallInt,
    Boolean,
    Bool,
    Varchar,
    Text,
    Float,
    Double,

    // Literals
    IntegerLit(i64),
    FloatLit(f64),
    StringLit(String),
    BooleanLit(bool),

    // Identifier
    Ident(String),

    // Symbols & Operators
    Equal,              // =
    NotEqual,           // != or <>
    LessThan,           // <
    LessThanOrEqual,    // <=
    GreaterThan,        // >
    GreaterThanOrEqual, // >=
    Plus,               // +
    Minus,              // -
    Star,               // *
    Slash,              // /
    Comma,              // ,
    Semicolon,          // ;
    LParen,             // (
    RParen,             // )

    Eof,
}
