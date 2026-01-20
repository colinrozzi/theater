//! S-expression parser
//!
//! Converts string input into SExpr values.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;

use crate::sexpr::SExpr;

/// Parse error with position information
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl ParseError {
    pub fn new(message: impl Into<String>, position: usize) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }
}

/// Tokenizer state
struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    LParen,
    RParen,
    Quote,
    Symbol(String),
    Number(i64),
    Float(f64),
    Str(String),
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() {
                self.next_char();
            } else if c == ';' {
                // Skip comment to end of line
                while let Some(c) = self.next_char() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, ParseError> {
        self.skip_whitespace();

        let c = match self.peek_char() {
            Some(c) => c,
            None => return Ok(None),
        };

        match c {
            '(' => {
                self.next_char();
                Ok(Some(Token::LParen))
            }
            ')' => {
                self.next_char();
                Ok(Some(Token::RParen))
            }
            '\'' => {
                self.next_char();
                Ok(Some(Token::Quote))
            }
            '"' => self.read_string(),
            _ if c.is_ascii_digit() || (c == '-' && self.is_number_ahead()) => self.read_number(),
            _ => self.read_symbol(),
        }
    }

    fn is_number_ahead(&self) -> bool {
        // Check if '-' is followed by a digit
        let rest = &self.input[self.pos..];
        let mut chars = rest.chars();
        if chars.next() == Some('-') {
            if let Some(c) = chars.next() {
                return c.is_ascii_digit();
            }
        }
        false
    }

    fn read_string(&mut self) -> Result<Option<Token>, ParseError> {
        let start_pos = self.pos;
        self.next_char(); // consume opening quote

        let mut s = String::new();
        loop {
            match self.next_char() {
                Some('"') => return Ok(Some(Token::Str(s))),
                Some('\\') => {
                    // Escape sequence
                    match self.next_char() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some(c) => {
                            return Err(ParseError::new(
                                alloc::format!("unknown escape sequence: \\{}", c),
                                self.pos,
                            ))
                        }
                        None => {
                            return Err(ParseError::new("unexpected end of string", self.pos))
                        }
                    }
                }
                Some(c) => s.push(c),
                None => return Err(ParseError::new("unterminated string", start_pos)),
            }
        }
    }

    fn read_number(&mut self) -> Result<Option<Token>, ParseError> {
        let start_pos = self.pos;
        let mut s = String::new();
        let mut has_dot = false;

        // Handle leading minus
        if self.peek_char() == Some('-') {
            s.push('-');
            self.next_char();
        }

        while let Some(c) = self.peek_char() {
            if c.is_ascii_digit() {
                s.push(c);
                self.next_char();
            } else if c == '.' && !has_dot {
                has_dot = true;
                s.push(c);
                self.next_char();
            } else if is_delimiter(c) {
                break;
            } else {
                // Not a pure number, might be a symbol like "123abc"
                // Read the rest as part of the symbol
                while let Some(c) = self.peek_char() {
                    if is_delimiter(c) {
                        break;
                    }
                    s.push(c);
                    self.next_char();
                }
                return Ok(Some(Token::Symbol(s)));
            }
        }

        if has_dot {
            match s.parse::<f64>() {
                Ok(f) => Ok(Some(Token::Float(f))),
                Err(_) => Err(ParseError::new(
                    alloc::format!("invalid float: {}", s),
                    start_pos,
                )),
            }
        } else {
            match s.parse::<i64>() {
                Ok(n) => Ok(Some(Token::Number(n))),
                Err(_) => Err(ParseError::new(
                    alloc::format!("invalid integer: {}", s),
                    start_pos,
                )),
            }
        }
    }

    fn read_symbol(&mut self) -> Result<Option<Token>, ParseError> {
        let mut s = String::new();

        while let Some(c) = self.peek_char() {
            if is_delimiter(c) {
                break;
            }
            s.push(c);
            self.next_char();
        }

        Ok(Some(Token::Symbol(s)))
    }
}

fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || c == '(' || c == ')' || c == '"' || c == '\'' || c == ';'
}

/// Parser state
struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Option<Token>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token()?;
        Ok(Self { lexer, current })
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        self.current = self.lexer.next_token()?;
        Ok(())
    }

    fn parse_expr(&mut self) -> Result<SExpr, ParseError> {
        match self.current.take() {
            Some(Token::LParen) => {
                self.advance()?;
                self.parse_list()
            }
            Some(Token::Quote) => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                // 'x -> (quote x)
                Ok(SExpr::List(alloc::vec![
                    Box::new(SExpr::Sym(String::from("quote"))),
                    Box::new(quoted),
                ]))
            }
            Some(Token::Symbol(s)) => {
                self.advance()?;
                if s == "nil" {
                    Ok(SExpr::Nil)
                } else {
                    Ok(SExpr::Sym(s))
                }
            }
            Some(Token::Number(n)) => {
                self.advance()?;
                Ok(SExpr::Num(n))
            }
            Some(Token::Float(f)) => {
                self.advance()?;
                Ok(SExpr::Flt(f))
            }
            Some(Token::Str(s)) => {
                self.advance()?;
                Ok(SExpr::Str(s))
            }
            Some(Token::RParen) => {
                Err(ParseError::new("unexpected ')'", self.lexer.pos))
            }
            None => {
                Err(ParseError::new("unexpected end of input", self.lexer.pos))
            }
        }
    }

    fn parse_list(&mut self) -> Result<SExpr, ParseError> {
        let mut items = Vec::new();

        loop {
            match &self.current {
                Some(Token::RParen) => {
                    self.advance()?;
                    return Ok(SExpr::List(items));
                }
                None => {
                    return Err(ParseError::new("unterminated list", self.lexer.pos));
                }
                _ => {
                    let expr = self.parse_expr()?;
                    items.push(Box::new(expr));
                }
            }
        }
    }
}

/// Parse a string into an S-expression
pub fn parse(input: &str) -> Result<SExpr, ParseError> {
    let mut parser = Parser::new(input)?;

    if parser.current.is_none() {
        return Ok(SExpr::Nil);
    }

    let expr = parser.parse_expr()?;

    // Check for trailing input
    if parser.current.is_some() {
        return Err(ParseError::new(
            "unexpected input after expression",
            parser.lexer.pos,
        ));
    }

    Ok(expr)
}

/// Parse multiple expressions (for files/programs)
#[allow(dead_code)]
pub fn parse_many(input: &str) -> Result<Vec<SExpr>, ParseError> {
    let mut parser = Parser::new(input)?;
    let mut exprs = Vec::new();

    while parser.current.is_some() {
        exprs.push(parser.parse_expr()?);
    }

    Ok(exprs)
}
