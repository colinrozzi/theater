//! S-expression type definition and Value conversion
//!
//! This module defines the SExpr type matching the WIT+ definition
//! and implements conversion to/from composite_guest::Value.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use composite_guest::Value;

/// S-expression - the core data type
///
/// Matches the WIT+ definition:
/// ```wit
/// variant sexpr {
///     sym(string),   // tag 0
///     num(s64),      // tag 1
///     flt(f64),      // tag 2
///     str(string),   // tag 3
///     list(list<self>), // tag 4
///     nil,           // tag 5
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum SExpr {
    /// A symbol (variable or function name)
    Sym(String),
    /// An integer
    Num(i64),
    /// A floating point number
    Flt(f64),
    /// A string literal
    Str(String),
    /// A list of S-expressions
    List(Vec<Box<SExpr>>),
    /// Nil / empty
    Nil,
}

impl SExpr {
    /// Check if this is nil
    pub fn is_nil(&self) -> bool {
        matches!(self, SExpr::Nil)
    }

    /// Check if this is truthy (everything except Nil is truthy)
    pub fn is_truthy(&self) -> bool {
        !matches!(self, SExpr::Nil)
    }

    /// Get as a list of SExprs (if it's a list)
    pub fn as_list(&self) -> Option<&[Box<SExpr>]> {
        match self {
            SExpr::List(items) => Some(items),
            _ => None,
        }
    }

    /// Convert to a displayable string
    pub fn display(&self) -> String {
        match self {
            SExpr::Sym(s) => s.clone(),
            SExpr::Num(n) => format!("{}", n),
            SExpr::Flt(f) => format!("{}", f),
            SExpr::Str(s) => format!("\"{}\"", s),
            SExpr::Nil => String::from("nil"),
            SExpr::List(items) => {
                let inner: Vec<String> = items.iter().map(|x| x.display()).collect();
                format!("({})", inner.join(" "))
            }
        }
    }
}

// Manual From/TryFrom implementations for SExpr <-> Value
// Required because of recursive Box<T> fields

impl From<SExpr> for Value {
    fn from(expr: SExpr) -> Value {
        match expr {
            SExpr::Sym(s) => Value::Variant {
                tag: 0,
                payload: Some(Box::new(Value::String(s))),
            },
            SExpr::Num(n) => Value::Variant {
                tag: 1,
                payload: Some(Box::new(Value::S64(n))),
            },
            SExpr::Flt(f) => Value::Variant {
                tag: 2,
                payload: Some(Box::new(Value::F64(f))),
            },
            SExpr::Str(s) => Value::Variant {
                tag: 3,
                payload: Some(Box::new(Value::String(s))),
            },
            SExpr::List(items) => {
                let values: Vec<Value> = items.into_iter().map(|x| (*x).into()).collect();
                Value::Variant {
                    tag: 4,
                    payload: Some(Box::new(Value::List(values))),
                }
            }
            SExpr::Nil => Value::Variant {
                tag: 5,
                payload: None,
            },
        }
    }
}

/// Conversion error
#[derive(Debug)]
pub struct ConversionError(pub String);

impl TryFrom<Value> for SExpr {
    type Error = ConversionError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Variant { tag, payload } => match tag {
                0 => {
                    // Sym(String)
                    let p = payload.ok_or_else(|| ConversionError("missing payload for sym".into()))?;
                    match *p {
                        Value::String(s) => Ok(SExpr::Sym(s)),
                        _ => Err(ConversionError("expected string for sym".into())),
                    }
                }
                1 => {
                    // Num(i64)
                    let p = payload.ok_or_else(|| ConversionError("missing payload for num".into()))?;
                    match *p {
                        Value::S64(n) => Ok(SExpr::Num(n)),
                        _ => Err(ConversionError("expected s64 for num".into())),
                    }
                }
                2 => {
                    // Flt(f64)
                    let p = payload.ok_or_else(|| ConversionError("missing payload for flt".into()))?;
                    match *p {
                        Value::F64(f) => Ok(SExpr::Flt(f)),
                        _ => Err(ConversionError("expected f64 for flt".into())),
                    }
                }
                3 => {
                    // Str(String)
                    let p = payload.ok_or_else(|| ConversionError("missing payload for str".into()))?;
                    match *p {
                        Value::String(s) => Ok(SExpr::Str(s)),
                        _ => Err(ConversionError("expected string for str".into())),
                    }
                }
                4 => {
                    // List(Vec<Box<SExpr>>)
                    let p = payload.ok_or_else(|| ConversionError("missing payload for list".into()))?;
                    match *p {
                        Value::List(items) => {
                            let mut result = Vec::with_capacity(items.len());
                            for item in items {
                                result.push(Box::new(SExpr::try_from(item)?));
                            }
                            Ok(SExpr::List(result))
                        }
                        _ => Err(ConversionError("expected list for list variant".into())),
                    }
                }
                5 => {
                    // Nil
                    Ok(SExpr::Nil)
                }
                _ => Err(ConversionError(format!("unknown tag: {}", tag))),
            },
            _ => Err(ConversionError("expected variant".into())),
        }
    }
}
