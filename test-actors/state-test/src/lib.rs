//! A simple state tracking test actor.
//!
//! This actor demonstrates typed state — no more option<list<u8>> serialization.
//! State is a pack Record passed directly between calls.
//!
//! - `init`: Takes empty tuple, returns initial state record
//! - `increment`: Takes state, returns updated state + new count
//! - `get-count`: Takes state, returns same state + current count

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

// Set up allocator and panic handler
packr_guest::setup_guest!();

// Embed interface metadata with typed state
pack_types! {
    record actor-state {
        count: s32,
    }

    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<actor-state, string>,
        theater:simple/state-test.increment: func(state: actor-state) -> result<tuple<actor-state, s32>, string>,
        theater:simple/state-test.get-count: func(state: actor-state) -> result<tuple<actor-state, s32>, string>,
    }
}

// Import the log function from the host
#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

/// Build a state record Value from a count
fn make_state(count: i32) -> Value {
    Value::Record {
        type_name: String::from("actor-state"),
        fields: vec![
            (String::from("count"), Value::S32(count)),
        ],
    }
}

/// Extract count from a state record Value
fn get_count_from_state(state: &Value) -> i32 {
    match state {
        Value::Record { fields, .. } => {
            for (name, val) in fields {
                if name == "count" {
                    if let Value::S32(n) = val {
                        return *n;
                    }
                }
            }
            0
        }
        _ => 0,
    }
}

fn ok_result(value: Value) -> Value {
    Value::Result {
        ok_type: value.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(value)),
    }
}

fn err_result(msg: &str) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(vec![]),
        err_type: ValueType::String,
        value: Err(Box::new(Value::String(String::from(msg)))),
    }
}

/// Initialize the actor — state arg is empty tuple on first call, return typed state
#[export(name = "theater:simple/actor.init")]
fn init(_input: Value) -> Value {
    log(String::from("state-test: init called"));

    // First element is state (empty tuple on first call), rest are params
    // For init, we ignore the incoming state and create fresh state
    let state = make_state(0);
    log(format!("state-test: initialized with count = 0"));

    // Init returns just the state (no extra return value)
    ok_result(state)
}

/// Increment the counter and return the new count
#[export(name = "theater:simple/state-test.increment")]
fn increment(state: Value) -> Value {
    let count = get_count_from_state(&state);
    let new_count = count + 1;

    log(format!("state-test: count incremented to {}", new_count));

    // Return tuple<new_state, return_value>
    ok_result(Value::Tuple(vec![make_state(new_count), Value::S32(new_count)]))
}

/// Get the current count without modifying state
#[export(name = "theater:simple/state-test.get-count")]
fn get_count(state: Value) -> Value {
    let count = get_count_from_state(&state);
    log(format!("state-test: current count = {}", count));

    // Return tuple<same_state, count>
    ok_result(Value::Tuple(vec![state.clone(), Value::S32(count)]))
}
