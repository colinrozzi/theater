//! Supervisor replay test actor for Pack runtime.
//!
//! A deterministic actor that exercises supervisor host functions:
//! - Imports `theater:simple/runtime.log`
//! - Imports `theater:simple/supervisor.{spawn, list-children, stop-child}`
//! - Exports `theater:simple/actor.init`
//! - Exports `theater:simple/message-server-client.handle-send`
//! - Exports `theater:simple/supervisor-handlers.handle-child-external-stop`
//!
//! Commands (sent as message bytes in handle-send):
//! - `"spawn:<manifest_path>"` → spawn a child, store child_id in state
//! - `"list"` → list children, log the count
//! - `"stop"` → read child_id from state, stop it
//!
//! Used to test supervisor lifecycle replay with hash verification.

#![no_std]

extern crate alloc;

use alloc::format;
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
        theater:simple/supervisor {
            spawn: func(manifest: string, init-bytes: option<list<u8>>, wasm-bytes: option<list<u8>>) -> result<string, string>,
            list-children: func() -> list<string>,
            stop-child: func(child-id: string) -> result<_, string>,
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-send: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/supervisor-handlers.handle-child-external-stop: func(state: option<list<u8>>, params: tuple<string>) -> result<tuple<option<list<u8>>>, string>,
    }
}

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/supervisor", name = "spawn")]
fn supervisor_spawn(manifest_path: String, init_bytes: Option<Vec<u8>>, wasm_bytes: Option<Vec<u8>>) -> Result<String, String>;

#[import(module = "theater:simple/supervisor", name = "list-children")]
fn supervisor_list_children() -> Vec<String>;

#[import(module = "theater:simple/supervisor", name = "stop-child")]
fn supervisor_stop_child(child_id: String) -> Result<(), String>;

#[import(module = "theater:simple/message-server-host", name = "register")]
fn message_server_register() -> Result<(), String>;

// ============================================================================
// State helpers
// ============================================================================

/// Store the child_id string as state bytes.
fn state_with_child_id(child_id: &str) -> Value {
    Value::Option {
        inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
        value: Some(alloc::boxed::Box::new(Value::List {
            elem_type: ValueType::U8,
            items: child_id.as_bytes().iter().map(|b| Value::U8(*b)).collect(),
        })),
    }
}

/// Extract the child_id string from state bytes.
fn child_id_from_state(state: &Value) -> Option<String> {
    match state {
        Value::Option {
            value: Some(inner), ..
        } => match inner.as_ref() {
            Value::List { items, .. } => {
                let bytes: Vec<u8> = items
                    .iter()
                    .filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None })
                    .collect();
                String::from_utf8(bytes).ok()
            }
            _ => None,
        },
        _ => None,
    }
}

// ============================================================================
// Actor exports
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            return err_result("Invalid input format");
        }
    };

    log(String::from("supervisor-replay-test: init called"));

    // Register with message server to receive commands
    if let Err(e) = message_server_register() {
        log(format!("supervisor-replay-test: register failed: {}", e));
        return err_result("Failed to register with message server");
    }
    log(String::from("supervisor-replay-test: registered with message server"));

    log(String::from("supervisor-replay-test: init complete"));

    ok_state(state)
}

#[export(name = "theater:simple/message-server-client.handle-send")]
fn handle_send(input: Value) -> Value {
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

    // Extract message bytes from params tuple: (source: string, data: list<u8>)
    let msg_bytes = match params {
        Value::Tuple(mut items) if items.len() >= 2 => extract_bytes(items.remove(1)),
        _ => alloc::vec![],
    };
    let msg = match core::str::from_utf8(&msg_bytes) {
        Ok(s) => s,
        Err(_) => {
            log(String::from("supervisor-replay-test: handle-send received non-utf8 data"));
            return ok_state(state);
        }
    };

    log(format!("supervisor-replay-test: handle-send: {}", msg));

    if let Some(manifest_path) = msg.strip_prefix("spawn:") {
        log(format!(
            "supervisor-replay-test: spawning child from {}",
            manifest_path
        ));
        match supervisor_spawn(String::from(manifest_path), None, None) {
            Ok(child_id) => {
                log(format!(
                    "supervisor-replay-test: spawned child {}",
                    child_id
                ));
                let new_state = state_with_child_id(&child_id);
                return ok_state(new_state);
            }
            Err(e) => {
                log(format!("supervisor-replay-test: spawn error: {}", e));
                return ok_state(state);
            }
        }
    } else if msg == "list" {
        log(String::from("supervisor-replay-test: listing children"));
        let children = supervisor_list_children();
        log(format!(
            "supervisor-replay-test: children count: {}",
            children.len()
        ));
        return ok_state(state);
    } else if msg == "stop" {
        match child_id_from_state(&state) {
            Some(child_id) => {
                log(format!(
                    "supervisor-replay-test: stopping child {}",
                    child_id
                ));
                match supervisor_stop_child(child_id) {
                    Ok(()) => {
                        log(String::from("supervisor-replay-test: stop-child succeeded"));
                    }
                    Err(e) => {
                        log(format!("supervisor-replay-test: stop error: {}", e));
                    }
                }
            }
            None => {
                log(String::from("supervisor-replay-test: no child_id in state"));
            }
        }
        return ok_state(state);
    }

    log(String::from("supervisor-replay-test: unknown command"));
    ok_state(state)
}

#[export(name = "theater:simple/supervisor-handlers.handle-child-external-stop")]
fn handle_child_external_stop(input: Value) -> Value {
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

    // params is Tuple([String(child_id)])
    let child_id = match params {
        Value::Tuple(mut items) if !items.is_empty() => match items.remove(0) {
            Value::String(s) => s,
            _ => String::from("unknown"),
        },
        _ => String::from("unknown"),
    };

    log(format!(
        "supervisor-replay-test: child externally stopped: {}",
        child_id
    ));

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
