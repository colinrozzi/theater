//! Replay test actor for Pack runtime (in-module state).
//!
//! A simple deterministic actor that:
//! - Imports `theater:simple/self.log`
//! - Exports `theater:simple/actor.init`
//! - Exports all 5 `theater:simple/message-server-client` handlers
//! - Calls `log` several times during init and when handling messages
//!
//! Used to test full lifecycle replay with hash verification. The `log` calls
//! carry STATIC string literals on purpose: they exercise the .rodata/static-data
//! marshalling path through the interceptor (numeric fixtures hide data bugs).
//!
//! State lives inside the module now (`docs/in-module-state.md`); this actor
//! holds none — every export just acts and returns. The handlers take only their
//! own params (no threaded state) and return only their own results.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types! {
    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(config: value) -> result<_, string>,
        theater:simple/message-server-client.handle-send: func(params: value) -> result<_, string>,
        theater:simple/message-server-client.handle-request: func(params: value) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-channel-open: func(params: value) -> result<tuple<tuple<bool, option<list<u8>>>>, string>,
        theater:simple/message-server-client.handle-channel-message: func(params: value) -> result<_, string>,
        theater:simple/message-server-client.handle-channel-close: func(params: value) -> result<_, string>,
    }
}

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/message-server-host", name = "register")]
fn message_server_register() -> Result<(), String>;

// ============================================================================
// Actor export: init
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
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

    ok_unit()
}

// ============================================================================
// Actor export: handle-send
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-send")]
fn handle_send(_params: Value) -> Value {
    log(String::from("Replay test actor: handle-send called"));
    log(String::from("Replay test actor: processing message"));

    ok_unit()
}

// ============================================================================
// Actor export: handle-request  (params: tuple<request-id, list<u8>>)
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-request")]
fn handle_request(params: Value) -> Value {
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

    // Return Ok((Some(response_bytes),))
    let response_option = Value::Option {
        inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
        value: Some(alloc::boxed::Box::new(Value::List {
            elem_type: ValueType::U8,
            items: response.into_iter().map(Value::U8).collect(),
        })),
    };

    let payload = Value::Tuple(alloc::vec![response_option]);
    Value::Result {
        ok_type: payload.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(payload)),
    }
}

// ============================================================================
// Actor export: handle-channel-open  (params: tuple<channel-id, list<u8>>)
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-channel-open")]
fn handle_channel_open(_params: Value) -> Value {
    log(String::from("Replay test actor: handle-channel-open called"));

    // Return Ok((channel-accept,))
    // channel-accept record encoded as Tuple([Bool(true), Option(None)])
    let channel_accept = Value::Tuple(alloc::vec![
        Value::Bool(true),
        Value::Option {
            inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
            value: None,
        },
    ]);

    let payload = Value::Tuple(alloc::vec![channel_accept]);
    Value::Result {
        ok_type: payload.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(payload)),
    }
}

// ============================================================================
// Actor export: handle-channel-message
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-channel-message")]
fn handle_channel_message(_params: Value) -> Value {
    log(String::from("Replay test actor: handle-channel-message called"));

    ok_unit()
}

// ============================================================================
// Actor export: handle-channel-close
// ============================================================================

#[export(name = "theater:simple/message-server-client.handle-channel-close")]
fn handle_channel_close(_params: Value) -> Value {
    log(String::from("Replay test actor: handle-channel-close called"));

    ok_unit()
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

/// `result<_, string>::ok(())` — no state to return.
fn ok_unit() -> Value {
    let unit = Value::Tuple(alloc::vec![]);
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(unit)),
    }
}
