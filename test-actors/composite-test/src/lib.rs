//! A simple test actor for Pack runtime integration with Theater.
//!
//! This actor:
//! 1. Exports `init` function for the theater:simple/actor interface
//! 2. Imports `log` function from theater:simple/runtime interface
//! 3. Demonstrates basic state handling

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

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
    }
}

// Import the log function from the host
#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

/// The init function for theater:simple/actor interface.
#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    // Extract state from input tuple
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("Composite test actor: init called!"));
    log(String::from("Composite test actor: init completed successfully!"));

    // Return the state unchanged, wrapped in Ok
    ok_state(state)
}

// ============================================================================
// Helpers
// ============================================================================

fn err_result(msg: &str) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(vec![]),
        err_type: ValueType::String,
        value: Err(alloc::boxed::Box::new(Value::String(String::from(msg)))),
    }
}

fn ok_state(state: Value) -> Value {
    let inner = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(inner)),
    }
}
