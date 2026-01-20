//! REPL Actor
//!
//! An interactive S-expression evaluator for the Theater runtime.
//!
//! This actor provides a request-response interface for evaluating
//! Lisp-like expressions. It supports:
//! - Basic arithmetic (+, -, *, /, %)
//! - Comparisons (=, <, >, <=, >=)
//! - Boolean operations (and, or, not)
//! - List operations (cons, car, cdr, list, length)
//! - Type predicates (nil?, num?, str?, sym?, list?)
//! - Special forms (quote, if)

#![no_std]

extern crate alloc;

mod sexpr;
mod parse;
mod eval;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use composite_guest::{export, import, Value};

use crate::parse::parse;
use crate::eval::eval;

// Set up allocator (1MB for complex expressions) and panic handler
composite_guest::setup_guest!(1024 * 1024);

// Import the log function from theater runtime
#[import(wit = "theater:simple/runtime.log")]
fn log(msg: String);

/// Initialize the actor
///
/// This is called by Theater when the actor starts. For the REPL actor,
/// we don't need any persistent state between restarts, so we just
/// pass through whatever state we're given.
#[export(wit = "theater:simple/actor.init")]
fn init(state: Option<Vec<u8>>) -> Result<(Option<Vec<u8>>,), String> {
    log(String::from("REPL actor initialized"));
    Ok((state,))
}

/// Evaluate an S-expression from a string input
///
/// This is the main REPL interface. It:
/// 1. Parses the input string into an SExpr
/// 2. Evaluates the expression
/// 3. Returns the result (or error)
///
/// The WIT signature is:
///   eval: func(input: string) -> eval-result
///
/// Where eval-result is:
///   variant eval-result { ok(sexpr), err(string) }
#[export(wit = "theater:simple/repl.eval")]
fn repl_eval(input: String) -> Value {
    log(format!("Evaluating: {}", input));

    // Parse the input
    let expr = match parse(&input) {
        Ok(e) => e,
        Err(e) => {
            let msg = format!("Parse error at position {}: {}", e.position, e.message);
            log(format!("Parse error: {}", msg));
            // Return eval-result::err(string)
            return Value::Variant {
                tag: 1, // err
                payload: Some(alloc::boxed::Box::new(Value::String(msg))),
            };
        }
    };

    log(format!("Parsed: {}", expr.display()));

    // Evaluate
    let result = match eval(&expr) {
        Ok(r) => r,
        Err(e) => {
            log(format!("Eval error: {}", e.0));
            // Return eval-result::err(string)
            return Value::Variant {
                tag: 1, // err
                payload: Some(alloc::boxed::Box::new(Value::String(e.0))),
            };
        }
    };

    log(format!("Result: {}", result.display()));

    // Return eval-result::ok(sexpr)
    Value::Variant {
        tag: 0, // ok
        payload: Some(alloc::boxed::Box::new(result.into())),
    }
}
