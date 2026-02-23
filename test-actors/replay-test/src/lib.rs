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
use pack_guest::{export, import, pack_types, Value, ValueType};

pack_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-send: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-request: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>, tuple<option<list<u8>>>>, string>,
        theater:simple/message-server-client.handle-channel-open: func(state: option<list<u8>>, params: tuple<string, option<list<u8>>>) -> result<tuple<option<list<u8>>, tuple<bool, option<list<u8>>>>, string>,
        theater:simple/message-server-client.handle-channel-message: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-channel-close: func(state: option<list<u8>>, params: tuple<string>) -> result<tuple<option<list<u8>>>, string>,
    }
}

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/message-server-host", name = "register")]
fn message_server_register() -> Result<(), String>;

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

    log(String::from("Replay test actor: init called"));
    log(String::from("Replay test actor: message 1"));
    log(String::from("Replay test actor: message 2"));
    log(String::from("Replay test actor: message 3"));

    // Register with message server to receive messages
    if let Err(e) = message_server_register() {
        log(alloc::format!("Replay test actor: register failed: {}", e));
        return err_result("Failed to register with message server");
    }
    log(String::from("Replay test actor: registered with message server"));

    log(String::from("Replay test actor: init complete"));

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

    log(String::from("Replay test actor: handle-send called"));
    log(String::from("Replay test actor: processing message"));

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

    log(String::from("Replay test actor: handle-request called"));

    // Extract data from params: tuple<string, list<u8>>
    let data_bytes = match params {
        Value::Tuple(mut items) if items.len() >= 2 => extract_bytes(items.remove(1)),
        _ => alloc::vec![],
    };

    log(String::from("Replay test actor: processing request"));

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

    log(String::from("Replay test actor: handle-channel-open called"));

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

    log(String::from("Replay test actor: handle-channel-message called"));

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

    log(String::from("Replay test actor: handle-channel-close called"));

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

/// Return an error result
fn err_result(msg: &str) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(alloc::vec![]),
        err_type: ValueType::String,
        value: Err(alloc::boxed::Box::new(Value::String(String::from(msg)))),
    }
}

/// Return an ok result wrapping the state tuple
fn ok_state(state: Value) -> Value {
    let inner = Value::Tuple(alloc::vec![state]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(inner)),
    }
}
