//! Replay test actor for Pack runtime.
//!
//! A simple deterministic actor that:
//! - Imports `theater:simple/runtime.log`
//! - Exports `theater:simple/actor.init`
//! - Exports all 5 `theater:simple/message-server-client` handlers
//! - Calls `log` several times during init and when handling messages
//!
//! Used to test full lifecycle replay with hash verification.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use pack_guest::{encode, export, Value, ValueType};

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
            return err_result("Invalid input format");
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
            return err_result("Invalid input format");
        }
    };

    log("Replay test actor: handle-send called");
    log("Replay test actor: processing message");

    // Return Ok((state,))
    ok_state(state)
}

// ============================================================================
// Actor export: handle-request
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-request")]
fn handle_request(input: Value) -> Value {
    // Extract (state, params) from input tuple
    let (state, params) = match input {
        Value::Tuple(mut items) if items.len() >= 2 => {
            let params = items.remove(1);
            let state = items.remove(0);
            (state, params)
        }
        _ => {
            return err_result("Invalid input format");
        }
    };

    log("Replay test actor: handle-request called");

    // Extract data from params: tuple<string, list<u8>>
    let data_bytes = match params {
        Value::Tuple(mut items) if items.len() >= 2 => extract_bytes(items.remove(1)),
        _ => alloc::vec![],
    };

    log("Replay test actor: processing request");

    // Build response: "response:" + data
    let mut response = alloc::vec::Vec::from(b"response:" as &[u8]);
    response.extend_from_slice(&data_bytes);

    // Return Ok((state, (Some(response_bytes),)))
    let response_option = Value::Option {
        inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
        value: Some(alloc::boxed::Box::new(Value::List {
            elem_type: ValueType::U8,
            items: response.into_iter().map(Value::U8).collect(),
        })),
    };

    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: alloc::vec![Value::Tuple(alloc::vec![
            state,
            Value::Tuple(alloc::vec![response_option]),
        ])],
    }
}

// ============================================================================
// Actor export: handle-channel-open
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-channel-open")]
fn handle_channel_open(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            return err_result("Invalid input format");
        }
    };

    log("Replay test actor: handle-channel-open called");

    // Return Ok((state, (channel-accept,)))
    // channel-accept record encoded as Tuple([Bool(true), Option(None)])
    let channel_accept = Value::Tuple(alloc::vec![
        Value::Bool(true),
        Value::Option {
            inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
            value: None,
        },
    ]);

    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: alloc::vec![Value::Tuple(alloc::vec![
            state,
            Value::Tuple(alloc::vec![channel_accept]),
        ])],
    }
}

// ============================================================================
// Actor export: handle-channel-message
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-channel-message")]
fn handle_channel_message(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            return err_result("Invalid input format");
        }
    };

    log("Replay test actor: handle-channel-message called");

    // Return Ok((state,))
    ok_state(state)
}

// ============================================================================
// Actor export: handle-channel-close
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-channel-close")]
fn handle_channel_close(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            return err_result("Invalid input format");
        }
    };

    log("Replay test actor: handle-channel-close called");

    // Return Ok((state,))
    ok_state(state)
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract bytes from a Value::List of U8
fn extract_bytes(value: Value) -> Vec<u8> {
    match value {
        Value::List { items, .. } => items
            .into_iter()
            .filter_map(|v| match v {
                Value::U8(b) => Some(b),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Return an error result variant
fn err_result(msg: &str) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("err"),
        tag: 1,
        payload: alloc::vec![Value::String(String::from(msg))],
    }
}

fn ok_state(state: Value) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: alloc::vec![Value::Tuple(alloc::vec![state])],
    }
}
