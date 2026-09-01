//! `supervisor` — spawn a child actor and learn when it dies.
//!
//! On init this actor spawns a child from a manifest (here the sibling `hello`
//! example) via `theater:simple/supervisor.spawn`. The supervisor auto-monitors
//! every actor it spawns, so when the child terminates the runtime invokes this
//! actor's `handle-lifecycle-event` with the child's terminal event — no manual
//! subscription needed. That's the whole supervision model in one actor.
//!
//! The `actor-info` / `spawn-failure` / `supervisor-error` types below mirror
//! `theater:simple/supervisor` exactly so the interface subset-hash matches the
//! host; get them wrong and the actor compiles but fails to instantiate.

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

pack_types! {
    record actor-info {
        id: string,
        name: string,
        parent-id: option<string>,
    }

    variant spawn-failure {
        bad-manifest(string),
        wasm-fetch(string),
        handler-registry(string),
        wasm-invalid(string),
        interface-mismatch(string),
        missing-interface(string),
        missing-metadata(string),
        init-failed(string),
        child-failed(string),
        child-stopped(string),
        timeout(string),
        internal(string),
    }

    variant supervisor-error {
        actor-not-found(string),
        out-of-view(string),
        permission-denied(string),
        invalid-argument(string),
        spawn-failed(spawn-failure),
        runtime-unavailable,
        internal(string),
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/supervisor {
            spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, supervisor-error>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/supervisor-handlers.handle-lifecycle-event: func(state: option<list<u8>>, params: tuple<string, string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/supervisor", name = "spawn")]
fn supervisor_spawn(
    manifest: String,
    init_state: Option<Value>,
    wasm_bytes: Option<alloc::vec::Vec<u8>>,
) -> Value;

fn empty_state() -> Value {
    Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: None,
    }
}

fn ok_state(state: Value) -> Value {
    let tuple = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: tuple.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(tuple)),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(_input: Value) -> Value {
    // Spawn the sibling `hello` example as our child. (Run this from the
    // `examples/` dir, with hello built, for the path to resolve.) `None` for
    // init-state and wasm-bytes — the packr-guest macro marshals Rust `Option`.
    match supervisor_spawn(String::from("hello/manifest.toml"), None, None) {
        Value::Variant { tag: 0, payload, .. } => match payload.into_iter().next() {
            Some(Value::String(id)) => log(format!("supervisor: spawned child {}", id)),
            _ => log(String::from("supervisor: spawned child (id unavailable)")),
        },
        _ => log(String::from("supervisor: spawn failed")),
    }
    ok_state(empty_state())
}

/// The runtime calls this when a watched child terminates. `params` is
/// (child-id, event-type, terminal-payload-bytes); we just log the death.
#[export(name = "theater:simple/supervisor-handlers.handle-lifecycle-event")]
fn handle_lifecycle_event(input: Value) -> Value {
    let (state, id) = match input {
        Value::Tuple(mut items) if items.len() >= 2 => {
            let id = match items.remove(1) {
                Value::String(s) => s,
                _ => String::from("<unknown>"),
            };
            (items.remove(0), id)
        }
        other => (other, String::from("<unknown>")),
    };
    log(format!("supervisor: my child {} terminated", id));
    ok_state(state)
}
