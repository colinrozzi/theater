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
use pack_guest::{decode, encode, export, Value, ValueType};

pack_guest::setup_guest!();

// ============================================================================
// Host imports
// ============================================================================

#[link(wasm_import_module = "theater:simple/runtime")]
extern "C" {
    #[link_name = "log"]
    fn host_log(in_ptr: i32, in_len: i32, out_ptr: i32, out_len: i32) -> i32;
}

#[link(wasm_import_module = "theater:simple/supervisor")]
extern "C" {
    #[link_name = "spawn"]
    fn host_spawn(in_ptr: i32, in_len: i32, out_ptr: i32, out_len: i32) -> i32;

    #[link_name = "list-children"]
    fn host_list_children(in_ptr: i32, in_len: i32, out_ptr: i32, out_len: i32) -> i32;

    #[link_name = "stop-child"]
    fn host_stop_child(in_ptr: i32, in_len: i32, out_ptr: i32, out_len: i32) -> i32;
}

// ============================================================================
// Host function helpers
// ============================================================================

fn log(msg: &str) {
    let input = Value::String(String::from(msg));
    let input_bytes = match encode(&input) {
        Ok(b) => b,
        Err(_) => return,
    };
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    unsafe {
        host_log(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            &mut out_ptr as *mut i32 as i32,
            &mut out_len as *mut i32 as i32,
        );
    }
}

/// Read the host function result from the output pointer/length slots.
fn read_host_result(out_ptr: i32, out_len: i32) -> Result<Value, String> {
    if out_len <= 0 {
        return Err(String::from("no result data"));
    }
    let result_bytes =
        unsafe { core::slice::from_raw_parts(out_ptr as *const u8, out_len as usize) };
    decode(result_bytes).map_err(|e| format!("decode: {:?}", e))
}

/// Extract Ok(String) from a result variant returned by a host function.
fn extract_ok_string(value: Value) -> Result<String, String> {
    match value {
        Value::Variant {
            tag: 0,
            mut payload,
            ..
        } => match payload.pop() {
            Some(Value::String(s)) => Ok(s),
            _ => Err(String::from("expected string in ok payload")),
        },
        Value::Variant {
            tag: 1,
            mut payload,
            ..
        } => match payload.pop() {
            Some(Value::String(s)) => Err(s),
            _ => Err(String::from("unknown error")),
        },
        _ => Err(String::from("expected result variant")),
    }
}

fn spawn_child(manifest: &str) -> Result<String, String> {
    let input = Value::Tuple(alloc::vec![
        Value::String(String::from(manifest)),
        Value::Option {
            inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
            value: None,
        },
    ]);
    let input_bytes = encode(&input).map_err(|e| format!("encode: {:?}", e))?;
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        host_spawn(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            &mut out_ptr as *mut i32 as i32,
            &mut out_len as *mut i32 as i32,
        )
    };
    if status != 0 {
        return Err(String::from("spawn host call failed"));
    }
    let result = read_host_result(out_ptr, out_len)?;
    extract_ok_string(result)
}

fn list_children_call() -> Result<(), String> {
    let input = Value::Tuple(alloc::vec![]);
    let input_bytes = encode(&input).map_err(|e| format!("encode: {:?}", e))?;
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        host_list_children(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            &mut out_ptr as *mut i32 as i32,
            &mut out_len as *mut i32 as i32,
        )
    };
    if status != 0 {
        return Err(String::from("list-children host call failed"));
    }
    let result = read_host_result(out_ptr, out_len)?;
    match result {
        Value::Variant {
            tag: 0, payload, ..
        } => {
            if let Some(Value::List { items, .. }) = payload.into_iter().next() {
                log(&format!(
                    "supervisor-replay-test: children count: {}",
                    items.len()
                ));
            }
            Ok(())
        }
        Value::Variant {
            tag: 1,
            mut payload,
            ..
        } => match payload.pop() {
            Some(Value::String(s)) => Err(s),
            _ => Err(String::from("list-children error")),
        },
        _ => Err(String::from("expected result variant")),
    }
}

fn stop_child_call(child_id: &str) -> Result<(), String> {
    let input = Value::String(String::from(child_id));
    let input_bytes = encode(&input).map_err(|e| format!("encode: {:?}", e))?;
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        host_stop_child(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            &mut out_ptr as *mut i32 as i32,
            &mut out_len as *mut i32 as i32,
        )
    };
    if status != 0 {
        return Err(String::from("stop-child host call failed"));
    }
    let result = read_host_result(out_ptr, out_len)?;
    match result {
        Value::Variant { tag: 0, .. } => Ok(()),
        Value::Variant {
            tag: 1,
            mut payload,
            ..
        } => match payload.pop() {
            Some(Value::String(s)) => Err(s),
            _ => Err(String::from("stop-child error")),
        },
        _ => Err(String::from("expected result variant")),
    }
}

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

    log("supervisor-replay-test: init called");
    log("supervisor-replay-test: init complete");

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

    // Extract message bytes from params (List<u8>)
    let msg_bytes = extract_bytes(params);
    let msg = match core::str::from_utf8(&msg_bytes) {
        Ok(s) => s,
        Err(_) => {
            log("supervisor-replay-test: handle-send received non-utf8 data");
            return ok_state(state);
        }
    };

    log(&format!("supervisor-replay-test: handle-send: {}", msg));

    if let Some(manifest_path) = msg.strip_prefix("spawn:") {
        log(&format!(
            "supervisor-replay-test: spawning child from {}",
            manifest_path
        ));
        match spawn_child(manifest_path) {
            Ok(child_id) => {
                log(&format!(
                    "supervisor-replay-test: spawned child {}",
                    child_id
                ));
                let new_state = state_with_child_id(&child_id);
                return ok_state(new_state);
            }
            Err(e) => {
                log(&format!("supervisor-replay-test: spawn error: {}", e));
                return ok_state(state);
            }
        }
    } else if msg == "list" {
        log("supervisor-replay-test: listing children");
        match list_children_call() {
            Ok(()) => {}
            Err(e) => {
                log(&format!("supervisor-replay-test: list error: {}", e));
            }
        }
        return ok_state(state);
    } else if msg == "stop" {
        match child_id_from_state(&state) {
            Some(child_id) => {
                log(&format!(
                    "supervisor-replay-test: stopping child {}",
                    child_id
                ));
                match stop_child_call(&child_id) {
                    Ok(()) => {
                        log("supervisor-replay-test: stop-child succeeded");
                    }
                    Err(e) => {
                        log(&format!("supervisor-replay-test: stop error: {}", e));
                    }
                }
            }
            None => {
                log("supervisor-replay-test: no child_id in state");
            }
        }
        return ok_state(state);
    }

    log("supervisor-replay-test: unknown command");
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

    log(&format!(
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

/// Return an error result variant
fn err_result(msg: &str) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("err"),
        tag: 1,
        payload: alloc::vec![Value::String(String::from(msg))],
    }
}

/// Return an ok result variant wrapping the state
fn ok_state(state: Value) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: alloc::vec![Value::Tuple(alloc::vec![state])],
    }
}
