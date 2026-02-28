//! Minimal shutdown test actor.
//!
//! This is the simplest possible actor for testing shutdown behavior.
//! It just logs on init and immediately calls shutdown.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

pack_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types!(file = "actor.types");

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/runtime", name = "shutdown")]
fn shutdown(data: Option<Vec<u8>>) -> Result<(), String>;

// ============================================================================
// Exports
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("shutdown-test: initializing"));
    log(String::from("shutdown-test: calling shutdown immediately"));

    // Shutdown with a result message
    let result_msg = b"shutdown-test completed".to_vec();
    let _ = shutdown(Some(result_msg));

    log(String::from("shutdown-test: shutdown called, returning"));

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
