//! Replay test actor for Pack runtime.
//!
//! A simple deterministic actor that:
//! - Imports `theater:simple/runtime.log`
//! - Exports `theater:simple/actor.init`
//! - Exports `theater:simple/message-server-client.handle-send`
//! - Calls `log` several times during init and when handling messages
//!
//! Used to test full lifecycle replay with hash verification.

#![no_std]

extern crate alloc;

use alloc::string::String;
use pack_guest::{encode, export, Value};

pack_guest::setup_guest!();

// ============================================================================
// Host imports
// ============================================================================

#[link(wasm_import_module = "theater:simple/runtime")]
extern "C" {
    #[link_name = "log"]
    fn host_log(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;
}

fn log(msg: &str) {
    let input = Value::String(String::from(msg));
    let input_bytes = match encode(&input) {
        Ok(b) => b,
        Err(_) => return,
    };
    let mut output_buf = [0u8; 64];
    unsafe {
        host_log(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            output_buf.as_mut_ptr() as i32,
            output_buf.len() as i32,
        );
    }
}

// ============================================================================
// Actor export: init
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    // Extract state from input tuple: (state, params)
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            return Value::Variant {
                type_name: String::from("result"),
                case_name: String::from("err"),
                tag: 1,
                payload: alloc::vec![Value::String(String::from("Invalid input format"))],
            };
        }
    };

    log("Replay test actor: init called");
    log("Replay test actor: message 1");
    log("Replay test actor: message 2");
    log("Replay test actor: message 3");
    log("Replay test actor: init complete");

    // Return Ok((state,))
    ok_state(state)
}

// ============================================================================
// Actor export: handle-send
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-send")]
fn handle_send(input: Value) -> Value {
    // Extract state from input tuple: (state, params)
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            return Value::Variant {
                type_name: String::from("result"),
                case_name: String::from("err"),
                tag: 1,
                payload: alloc::vec![Value::String(String::from("Invalid input format"))],
            };
        }
    };

    log("Replay test actor: handle-send called");
    log("Replay test actor: processing message");

    // Return Ok((state,))
    ok_state(state)
}

// ============================================================================
// Helpers
// ============================================================================

fn ok_state(state: Value) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: alloc::vec![Value::Tuple(alloc::vec![state])],
    }
}
