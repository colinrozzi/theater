//! Supervisor replay test actor for Pack runtime.
//!
//! A deterministic actor that exercises supervisor host functions:
//! - Imports `theater:simple/self.log`
//! - Imports `theater:simple/supervisor.{spawn, list-actors, stop-actor}`
//! - Exports `theater:simple/actor.init`
//! - Exports `theater:simple/message-server-client.handle-send`
//! - Exports `theater:simple/supervisor-handlers.handle-actor-external-stop`
//!
//! Commands (sent as message bytes in handle-send):
//! - `"spawn:<manifest_path>"` → spawn a child, store child_id in state
//! - `"list"` → list actors in view, log the count
//! - `"stop"` → read child_id from state, stop it
//!
//! Used to test supervisor lifecycle replay with hash verification.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

// Embed interface metadata for hash verification. The supervisor types below
// mirror theater:simple/supervisor exactly (field/case names + order) so the
// host's interface subset hash matches — get this wrong and the actor compiles
// but fails to instantiate.
pack_types! {
    record actor-info {
        id: string,
        name: string,
        parent-id: option<string>,
    }

    variant supervisor-error {
        actor-not-found(string),
        out-of-view(string),
        permission-denied(string),
        invalid-argument(string),
        spawn-failed(string),
        runtime-unavailable,
        internal(string),
        handler-registry-failed(string),
        wasm-invalid(string),
        interface-mismatch(string),
        missing-interface(string),
        missing-metadata(string),
        init-failed(string),
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/supervisor {
            spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, supervisor-error>,
            list-actors: func() -> result<list<actor-info>, supervisor-error>,
            stop-actor: func(id: string) -> result<_, supervisor-error>,
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-send: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/supervisor-handlers.handle-actor-external-stop: func(state: option<list<u8>>, params: tuple<string>) -> result<tuple<option<list<u8>>>, string>,
    }
}

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

// The supervisor ops return `result<T, supervisor-error>`; import them raw and
// parse the Value so the actor can react to the structured error case.
#[import(module = "theater:simple/supervisor", name = "spawn")]
fn supervisor_spawn_raw(
    manifest_path: String,
    init_state: Option<Value>,
    wasm_bytes: Option<Vec<u8>>,
) -> Value;

#[import(module = "theater:simple/supervisor", name = "list-actors")]
fn supervisor_list_actors_raw() -> Value;

#[import(module = "theater:simple/supervisor", name = "stop-actor")]
fn supervisor_stop_actor_raw(id: String) -> Value;

/// Render a `supervisor-error` variant Value as its case name.
fn supervisor_error_case(v: Option<Value>) -> String {
    match v {
        Some(Value::Variant { case_name, .. }) => case_name,
        _ => String::from("unknown"),
    }
}

/// spawn -> Ok(child-id) | Err(case-name)
fn supervisor_spawn(manifest_path: String) -> Result<String, String> {
    match supervisor_spawn_raw(manifest_path, None, None) {
        Value::Variant { tag: 0, payload, .. } => match payload.into_iter().next() {
            Some(Value::String(id)) => Ok(id),
            _ => Err(String::from("unexpected ok payload")),
        },
        Value::Variant { tag: 1, payload, .. } => {
            Err(supervisor_error_case(payload.into_iter().next()))
        }
        _ => Err(String::from("unexpected result format")),
    }
}

/// list-actors -> Ok(count) | Err(case-name)
fn supervisor_list_actors_count() -> Result<usize, String> {
    match supervisor_list_actors_raw() {
        Value::Variant { tag: 0, payload, .. } => match payload.into_iter().next() {
            Some(Value::List { items, .. }) => Ok(items.len()),
            _ => Err(String::from("unexpected ok payload")),
        },
        Value::Variant { tag: 1, payload, .. } => {
            Err(supervisor_error_case(payload.into_iter().next()))
        }
        _ => Err(String::from("unexpected result format")),
    }
}

/// stop-actor -> Ok(()) | Err(case-name)
fn supervisor_stop_actor(id: String) -> Result<(), String> {
    match supervisor_stop_actor_raw(id) {
        Value::Variant { tag: 0, .. } => Ok(()),
        Value::Variant { tag: 1, payload, .. } => {
            Err(supervisor_error_case(payload.into_iter().next()))
        }
        _ => Err(String::from("unexpected result format")),
    }
}

#[import(module = "theater:simple/message-server-host", name = "register")]
fn message_server_register() -> Result<(), String>;

// ============================================================================
// State helpers
// ============================================================================

/// An empty actor state (`none`), used when init receives no prior state.
fn empty_state() -> Value {
    Value::Option {
        inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
        value: None,
    }
}

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
    // The runtime calls init with an empty params tuple; there is no prior
    // state on first init, so default to an empty (none) state.
    let state = match input {
        Value::Tuple(mut items) if !items.is_empty() => items.remove(0),
        _ => empty_state(),
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
        match supervisor_spawn(String::from(manifest_path)) {
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
        log(String::from("supervisor-replay-test: listing actors in view"));
        match supervisor_list_actors_count() {
            Ok(n) => log(format!("supervisor-replay-test: actor count: {}", n)),
            Err(e) => log(format!("supervisor-replay-test: list error: {}", e)),
        }
        return ok_state(state);
    } else if msg == "stop" {
        match child_id_from_state(&state) {
            Some(child_id) => {
                log(format!(
                    "supervisor-replay-test: stopping child {}",
                    child_id
                ));
                match supervisor_stop_actor(child_id) {
                    Ok(()) => {
                        log(String::from("supervisor-replay-test: stop-actor succeeded"));
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

#[export(name = "theater:simple/supervisor-handlers.handle-actor-external-stop")]
fn handle_actor_external_stop(input: Value) -> Value {
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
