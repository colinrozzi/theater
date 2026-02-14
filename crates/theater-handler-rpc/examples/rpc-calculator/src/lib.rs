//! RPC Calculator Actor
//!
//! A simple actor that exports math functions that can be called via RPC.
//!
//! Exports:
//! - `my:calculator.add(a: i32, b: i32) -> i32`
//! - `my:calculator.subtract(a: i32, b: i32) -> i32`
//! - `my:calculator.multiply(a: i32, b: i32) -> i32`
//! - `my:calculator.divide(a: i32, b: i32) -> result<i32, string>`

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use pack_guest::{export, import, Value};

pack_guest::setup_guest!();

pack_guest::pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(input: value) -> value,
        my:calculator.add: func(input: value) -> value,
        my:calculator.subtract: func(input: value) -> value,
        my:calculator.multiply: func(input: value) -> value,
        my:calculator.divide: func(input: value) -> value,
    }
}

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

// ============================================================================
// Exports - Calculator functions
// ============================================================================

/// Initialize the actor
#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("calculator: initialized"));
    ok_state(state)
}

/// Add two numbers: add(a: i32, b: i32) -> i32
#[export(name = "my:calculator.add")]
fn add(input: Value) -> Value {
    // Input is (state, params) where params = tuple<a: i32, b: i32>
    let (state, a, b) = match parse_binary_op_input(&input) {
        Ok(v) => v,
        Err(e) => return err_result(&e),
    };

    let result = a + b;
    log(String::from("calculator: add"));

    ok_result_with_state(state, Value::S32(result))
}

/// Subtract two numbers: subtract(a: i32, b: i32) -> i32
#[export(name = "my:calculator.subtract")]
fn subtract(input: Value) -> Value {
    let (state, a, b) = match parse_binary_op_input(&input) {
        Ok(v) => v,
        Err(e) => return err_result(&e),
    };

    let result = a - b;
    log(String::from("calculator: subtract"));

    ok_result_with_state(state, Value::S32(result))
}

/// Multiply two numbers: multiply(a: i32, b: i32) -> i32
#[export(name = "my:calculator.multiply")]
fn multiply(input: Value) -> Value {
    let (state, a, b) = match parse_binary_op_input(&input) {
        Ok(v) => v,
        Err(e) => return err_result(&e),
    };

    let result = a * b;
    log(String::from("calculator: multiply"));

    ok_result_with_state(state, Value::S32(result))
}

/// Divide two numbers: divide(a: i32, b: i32) -> result<i32, string>
#[export(name = "my:calculator.divide")]
fn divide(input: Value) -> Value {
    let (state, a, b) = match parse_binary_op_input(&input) {
        Ok(v) => v,
        Err(e) => return err_result(&e),
    };

    if b == 0 {
        log(String::from("calculator: divide by zero!"));
        // Return error result for divide by zero
        let inner_result = Value::Variant {
            type_name: String::from("result"),
            case_name: String::from("err"),
            tag: 1,
            payload: vec![Value::String(String::from("Division by zero"))],
        };
        return ok_result_with_state(state, inner_result);
    }

    let result = a / b;
    log(String::from("calculator: divide"));

    // Return ok result with the quotient
    let inner_result = Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: vec![Value::S32(result)],
    };
    ok_result_with_state(state, inner_result)
}

// ============================================================================
// Helpers
// ============================================================================

/// Parse input for binary operations: (state, (a, b))
fn parse_binary_op_input(input: &Value) -> Result<(Value, i32, i32), String> {
    match input {
        Value::Tuple(items) if items.len() >= 2 => {
            let state = items[0].clone();
            let params = &items[1];

            let (a, b) = match params {
                Value::Tuple(p) if p.len() >= 2 => {
                    let a = match &p[0] {
                        Value::S32(v) => *v,
                        _ => return Err(String::from("Expected i32 for first argument")),
                    };
                    let b = match &p[1] {
                        Value::S32(v) => *v,
                        _ => return Err(String::from("Expected i32 for second argument")),
                    };
                    (a, b)
                }
                _ => return Err(String::from("Expected tuple of two i32 values")),
            };

            Ok((state, a, b))
        }
        _ => Err(String::from("Invalid input format")),
    }
}

fn err_result(msg: &str) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("err"),
        tag: 1,
        payload: vec![Value::String(String::from(msg))],
    }
}

fn ok_state(state: Value) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: vec![Value::Tuple(vec![state])],
    }
}

fn ok_result_with_state(state: Value, result: Value) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: vec![Value::Tuple(vec![state, result])],
    }
}
