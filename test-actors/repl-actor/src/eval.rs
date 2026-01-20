//! S-expression evaluator
//!
//! A simple interpreter for S-expressions with built-in functions.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use alloc::vec;

use crate::sexpr::SExpr;

/// Evaluation error
#[derive(Debug)]
pub struct EvalError(pub String);

impl EvalError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

/// Evaluate an S-expression
pub fn eval(expr: &SExpr) -> Result<SExpr, EvalError> {
    match expr {
        // Self-evaluating forms
        SExpr::Num(_) | SExpr::Flt(_) | SExpr::Str(_) | SExpr::Nil => Ok(expr.clone()),

        // Symbols evaluate to themselves for now (no environment yet)
        SExpr::Sym(_) => Ok(expr.clone()),

        // Function application
        SExpr::List(items) => {
            if items.is_empty() {
                return Ok(SExpr::Nil);
            }

            let head = &items[0];
            let args = &items[1..];

            match head.as_ref() {
                SExpr::Sym(name) => apply_builtin(name, args),
                _ => Err(EvalError::new("first element must be a symbol")),
            }
        }
    }
}

/// Apply a built-in function
fn apply_builtin(name: &str, args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    match name {
        // Special forms (don't evaluate args)
        "quote" => builtin_quote(args),
        "if" => builtin_if(args),

        // Arithmetic
        "+" | "add" => builtin_add(args),
        "-" | "sub" => builtin_sub(args),
        "*" | "mul" => builtin_mul(args),
        "/" | "div" => builtin_div(args),
        "%" | "mod" => builtin_mod(args),

        // Comparison
        "=" | "eq" => builtin_eq(args),
        "<" => builtin_lt(args),
        ">" => builtin_gt(args),
        "<=" => builtin_lte(args),
        ">=" => builtin_gte(args),

        // Boolean
        "not" => builtin_not(args),
        "and" => builtin_and(args),
        "or" => builtin_or(args),

        // List operations
        "cons" => builtin_cons(args),
        "car" | "first" | "head" => builtin_car(args),
        "cdr" | "rest" | "tail" => builtin_cdr(args),
        "list" => builtin_list(args),
        "len" | "length" => builtin_length(args),

        // Type predicates
        "nil?" => builtin_is_nil(args),
        "num?" => builtin_is_num(args),
        "str?" => builtin_is_str(args),
        "sym?" => builtin_is_sym(args),
        "list?" => builtin_is_list(args),

        _ => Err(EvalError::new(format!("unknown function: {}", name))),
    }
}

// =============================================================================
// Special forms
// =============================================================================

fn builtin_quote(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("quote requires exactly 1 argument"));
    }
    Ok((*args[0]).clone())
}

fn builtin_if(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::new("if requires 2 or 3 arguments"));
    }

    let cond = eval(&args[0])?;
    if cond.is_truthy() {
        eval(&args[1])
    } else if args.len() == 3 {
        eval(&args[2])
    } else {
        Ok(SExpr::Nil)
    }
}

// =============================================================================
// Arithmetic
// =============================================================================

fn builtin_add(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    let mut sum: i64 = 0;
    let mut is_float = false;
    let mut float_sum: f64 = 0.0;

    for arg in args {
        let val = eval(arg)?;
        match val {
            SExpr::Num(n) => {
                if is_float {
                    float_sum += n as f64;
                } else {
                    sum += n;
                }
            }
            SExpr::Flt(f) => {
                if !is_float {
                    is_float = true;
                    float_sum = sum as f64;
                }
                float_sum += f;
            }
            _ => return Err(EvalError::new("+ requires numbers")),
        }
    }

    if is_float {
        Ok(SExpr::Flt(float_sum))
    } else {
        Ok(SExpr::Num(sum))
    }
}

fn builtin_sub(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.is_empty() {
        return Err(EvalError::new("- requires at least one argument"));
    }

    let first = eval(&args[0])?;

    // Unary negation
    if args.len() == 1 {
        return match first {
            SExpr::Num(n) => Ok(SExpr::Num(-n)),
            SExpr::Flt(f) => Ok(SExpr::Flt(-f)),
            _ => Err(EvalError::new("- requires numbers")),
        };
    }

    let (mut result, mut is_float) = match first {
        SExpr::Num(n) => (n as f64, false),
        SExpr::Flt(f) => (f, true),
        _ => return Err(EvalError::new("- requires numbers")),
    };

    for arg in &args[1..] {
        let val = eval(arg)?;
        match val {
            SExpr::Num(n) => result -= n as f64,
            SExpr::Flt(f) => {
                is_float = true;
                result -= f;
            }
            _ => return Err(EvalError::new("- requires numbers")),
        }
    }

    if is_float {
        Ok(SExpr::Flt(result))
    } else {
        Ok(SExpr::Num(result as i64))
    }
}

fn builtin_mul(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    let mut product: i64 = 1;
    let mut is_float = false;
    let mut float_product: f64 = 1.0;

    for arg in args {
        let val = eval(arg)?;
        match val {
            SExpr::Num(n) => {
                if is_float {
                    float_product *= n as f64;
                } else {
                    product *= n;
                }
            }
            SExpr::Flt(f) => {
                if !is_float {
                    is_float = true;
                    float_product = product as f64;
                }
                float_product *= f;
            }
            _ => return Err(EvalError::new("* requires numbers")),
        }
    }

    if is_float {
        Ok(SExpr::Flt(float_product))
    } else {
        Ok(SExpr::Num(product))
    }
}

fn builtin_div(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new("/ requires exactly 2 arguments"));
    }

    let a = eval(&args[0])?;
    let b = eval(&args[1])?;

    match (&a, &b) {
        (SExpr::Num(x), SExpr::Num(y)) => {
            if *y == 0 {
                Err(EvalError::new("division by zero"))
            } else {
                Ok(SExpr::Num(x / y))
            }
        }
        (SExpr::Num(x), SExpr::Flt(y)) => {
            if *y == 0.0 {
                Err(EvalError::new("division by zero"))
            } else {
                Ok(SExpr::Flt(*x as f64 / y))
            }
        }
        (SExpr::Flt(x), SExpr::Num(y)) => {
            if *y == 0 {
                Err(EvalError::new("division by zero"))
            } else {
                Ok(SExpr::Flt(x / *y as f64))
            }
        }
        (SExpr::Flt(x), SExpr::Flt(y)) => {
            if *y == 0.0 {
                Err(EvalError::new("division by zero"))
            } else {
                Ok(SExpr::Flt(x / y))
            }
        }
        _ => Err(EvalError::new("/ requires numbers")),
    }
}

fn builtin_mod(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new("% requires exactly 2 arguments"));
    }

    let a = eval(&args[0])?;
    let b = eval(&args[1])?;

    match (&a, &b) {
        (SExpr::Num(x), SExpr::Num(y)) => {
            if *y == 0 {
                Err(EvalError::new("modulo by zero"))
            } else {
                Ok(SExpr::Num(x % y))
            }
        }
        _ => Err(EvalError::new("% requires integers")),
    }
}

// =============================================================================
// Comparison
// =============================================================================

fn builtin_eq(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new("= requires exactly 2 arguments"));
    }

    let a = eval(&args[0])?;
    let b = eval(&args[1])?;

    Ok(if a == b { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_lt(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new("< requires exactly 2 arguments"));
    }

    let a = eval(&args[0])?;
    let b = eval(&args[1])?;

    let result = match (&a, &b) {
        (SExpr::Num(x), SExpr::Num(y)) => x < y,
        (SExpr::Flt(x), SExpr::Flt(y)) => x < y,
        (SExpr::Num(x), SExpr::Flt(y)) => (*x as f64) < *y,
        (SExpr::Flt(x), SExpr::Num(y)) => *x < (*y as f64),
        _ => return Err(EvalError::new("< requires numbers")),
    };

    Ok(if result { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_gt(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new("> requires exactly 2 arguments"));
    }

    let a = eval(&args[0])?;
    let b = eval(&args[1])?;

    let result = match (&a, &b) {
        (SExpr::Num(x), SExpr::Num(y)) => x > y,
        (SExpr::Flt(x), SExpr::Flt(y)) => x > y,
        (SExpr::Num(x), SExpr::Flt(y)) => (*x as f64) > *y,
        (SExpr::Flt(x), SExpr::Num(y)) => *x > (*y as f64),
        _ => return Err(EvalError::new("> requires numbers")),
    };

    Ok(if result { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_lte(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new("<= requires exactly 2 arguments"));
    }

    let a = eval(&args[0])?;
    let b = eval(&args[1])?;

    let result = match (&a, &b) {
        (SExpr::Num(x), SExpr::Num(y)) => x <= y,
        (SExpr::Flt(x), SExpr::Flt(y)) => x <= y,
        (SExpr::Num(x), SExpr::Flt(y)) => (*x as f64) <= *y,
        (SExpr::Flt(x), SExpr::Num(y)) => *x <= (*y as f64),
        _ => return Err(EvalError::new("<= requires numbers")),
    };

    Ok(if result { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_gte(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new(">= requires exactly 2 arguments"));
    }

    let a = eval(&args[0])?;
    let b = eval(&args[1])?;

    let result = match (&a, &b) {
        (SExpr::Num(x), SExpr::Num(y)) => x >= y,
        (SExpr::Flt(x), SExpr::Flt(y)) => x >= y,
        (SExpr::Num(x), SExpr::Flt(y)) => (*x as f64) >= *y,
        (SExpr::Flt(x), SExpr::Num(y)) => *x >= (*y as f64),
        _ => return Err(EvalError::new(">= requires numbers")),
    };

    Ok(if result { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

// =============================================================================
// Boolean
// =============================================================================

fn builtin_not(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("not requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    Ok(if val.is_truthy() { SExpr::Nil } else { SExpr::Sym(String::from("true")) })
}

fn builtin_and(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    for arg in args {
        let val = eval(arg)?;
        if !val.is_truthy() {
            return Ok(SExpr::Nil);
        }
    }
    Ok(SExpr::Sym(String::from("true")))
}

fn builtin_or(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    for arg in args {
        let val = eval(arg)?;
        if val.is_truthy() {
            return Ok(SExpr::Sym(String::from("true")));
        }
    }
    Ok(SExpr::Nil)
}

// =============================================================================
// List operations
// =============================================================================

fn builtin_cons(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::new("cons requires exactly 2 arguments"));
    }

    let head = eval(&args[0])?;
    let tail = eval(&args[1])?;

    match tail {
        SExpr::List(mut items) => {
            items.insert(0, Box::new(head));
            Ok(SExpr::List(items))
        }
        SExpr::Nil => Ok(SExpr::List(vec![Box::new(head)])),
        _ => Err(EvalError::new("cons: second argument must be a list or nil")),
    }
}

fn builtin_car(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("car requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    match val {
        SExpr::List(items) if !items.is_empty() => Ok((*items[0]).clone()),
        SExpr::List(_) => Err(EvalError::new("car of empty list")),
        SExpr::Nil => Err(EvalError::new("car of nil")),
        _ => Err(EvalError::new("car requires a list")),
    }
}

fn builtin_cdr(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("cdr requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    match val {
        SExpr::List(mut items) if !items.is_empty() => {
            items.remove(0);
            if items.is_empty() {
                Ok(SExpr::Nil)
            } else {
                Ok(SExpr::List(items))
            }
        }
        SExpr::List(_) => Err(EvalError::new("cdr of empty list")),
        SExpr::Nil => Err(EvalError::new("cdr of nil")),
        _ => Err(EvalError::new("cdr requires a list")),
    }
}

fn builtin_list(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    let mut result = Vec::with_capacity(args.len());
    for arg in args {
        result.push(Box::new(eval(arg)?));
    }
    Ok(SExpr::List(result))
}

fn builtin_length(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("length requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    match val {
        SExpr::List(items) => Ok(SExpr::Num(items.len() as i64)),
        SExpr::Nil => Ok(SExpr::Num(0)),
        SExpr::Str(s) => Ok(SExpr::Num(s.len() as i64)),
        _ => Err(EvalError::new("length requires a list or string")),
    }
}

// =============================================================================
// Type predicates
// =============================================================================

fn builtin_is_nil(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("nil? requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    Ok(if val.is_nil() { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_is_num(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("num? requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    let is_num = matches!(val, SExpr::Num(_) | SExpr::Flt(_));
    Ok(if is_num { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_is_str(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("str? requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    let is_str = matches!(val, SExpr::Str(_));
    Ok(if is_str { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_is_sym(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("sym? requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    let is_sym = matches!(val, SExpr::Sym(_));
    Ok(if is_sym { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}

fn builtin_is_list(args: &[Box<SExpr>]) -> Result<SExpr, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new("list? requires exactly 1 argument"));
    }

    let val = eval(&args[0])?;
    let is_list = matches!(val, SExpr::List(_) | SExpr::Nil);
    Ok(if is_list { SExpr::Sym(String::from("true")) } else { SExpr::Nil })
}
