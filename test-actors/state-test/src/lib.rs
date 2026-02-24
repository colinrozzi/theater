//! A simple state tracking test actor.
//!
//! This actor demonstrates and tests state persistence across function calls:
//! - `init`: Initialize with {"count": 0} or use provided state
//! - `increment`: Increment the counter, return new count
//! - `get-count`: Return current count without modifying state
//!
//! Used for golden file testing to verify state tracking works correctly.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use pack_guest::{export, import, pack_types, Value, ValueType};
use serde::{Deserialize, Serialize};

// Set up allocator and panic handler
pack_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/state-test.increment: func(state: option<list<u8>>, params: tuple<>) -> result<tuple<option<list<u8>>, s32>, string>,
        theater:simple/state-test.get-count: func(state: option<list<u8>>, params: tuple<>) -> result<tuple<option<list<u8>>, s32>, string>,
    }
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    count: i32,
}

// Import the log function from the host
#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

/// Initialize the actor with state
#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    // Extract state from input tuple
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("state-test: init called"));

    // Parse existing state or create default
    let current_state: State = match extract_state_bytes(&state) {
        Some(bytes) => {
            match serde_json::from_slice(&bytes) {
                Ok(s) => {
                    log(String::from("state-test: loaded existing state"));
                    s
                }
                Err(_) => {
                    log(String::from("state-test: creating default state"));
                    State::default()
                }
            }
        }
        None => {
            log(String::from("state-test: no state provided, creating default"));
            State::default()
        }
    };

    log(format!("state-test: initial count = {}", current_state.count));

    // Return the state
    ok_state(serialize_state(&current_state))
}

/// Increment the counter and return the new count
#[export(name = "theater:simple/state-test.increment")]
fn increment(input: Value) -> Value {
    // Extract state and params from input tuple
    let (state, _params) = match extract_state_and_params(input) {
        Ok(v) => v,
        Err(e) => return e,
    };

    log(String::from("state-test: increment called"));

    // Parse state
    let mut current_state: State = match extract_state_bytes(&state) {
        Some(bytes) => {
            serde_json::from_slice(&bytes).unwrap_or_default()
        }
        None => State::default(),
    };

    // Increment
    current_state.count += 1;
    log(format!("state-test: count incremented to {}", current_state.count));

    // Return new state and the new count
    ok_state_with_output(serialize_state(&current_state), Value::S32(current_state.count))
}

/// Get the current count without modifying state
#[export(name = "theater:simple/state-test.get-count")]
fn get_count(input: Value) -> Value {
    // Extract state and params from input tuple
    let (state, _params) = match extract_state_and_params(input) {
        Ok(v) => v,
        Err(e) => return e,
    };

    log(String::from("state-test: get-count called"));

    // Parse state
    let current_state: State = match extract_state_bytes(&state) {
        Some(bytes) => {
            serde_json::from_slice(&bytes).unwrap_or_default()
        }
        None => State::default(),
    };

    log(format!("state-test: current count = {}", current_state.count));

    // Return unchanged state and the count
    ok_state_with_output(state, Value::S32(current_state.count))
}

// ============================================================================
// Helpers
// ============================================================================

fn extract_state_and_params(input: Value) -> Result<(Value, Value), Value> {
    match input {
        Value::Tuple(mut items) if items.len() >= 2 => {
            let state = items.remove(0);
            let params = items.remove(0);
            Ok((state, params))
        }
        Value::Tuple(mut items) if items.len() == 1 => {
            let state = items.remove(0);
            Ok((state, Value::Tuple(vec![])))
        }
        _ => Err(err_result("Invalid input format: expected tuple")),
    }
}

fn extract_state_bytes(state: &Value) -> Option<Vec<u8>> {
    match state {
        Value::Option { value: Some(inner), .. } => {
            match inner.as_ref() {
                Value::List { items, .. } => {
                    Some(items.iter().filter_map(|v| {
                        if let Value::U8(b) = v { Some(*b) } else { None }
                    }).collect())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn serialize_state(state: &State) -> Value {
    let bytes = serde_json::to_vec(state).unwrap_or_default();
    Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: Some(Box::new(Value::List {
            elem_type: ValueType::U8,
            items: bytes.into_iter().map(Value::U8).collect(),
        })),
    }
}

fn err_result(msg: &str) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(vec![]),
        err_type: ValueType::String,
        value: Err(Box::new(Value::String(String::from(msg)))),
    }
}

fn ok_state(state: Value) -> Value {
    let inner = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(inner)),
    }
}

fn ok_state_with_output(state: Value, output: Value) -> Value {
    let inner = Value::Tuple(vec![state, output]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(inner)),
    }
}
