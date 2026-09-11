//! `supervisor` — spawn a child actor and learn when it dies.
//!
//! On init this actor spawns a child from a manifest (here the sibling `hello`
//! example) via `theater:simple/runtime.spawn`, then `theater:simple/lifecycle.monitor`s
//! it. When the child terminates the lifecycle handler invokes this actor's
//! `handle-actor-event` with the child's terminal event. That's the whole
//! supervision model in one actor now that actor-management is a runtime
//! primitive (there is no separate supervisor interface, and spawn no longer
//! auto-monitors — you attach a monitor explicitly). State lives inside the
//! module now (`docs/in-module-state.md`); this actor holds none.
//!
//! The `spawn-failure` / `runtime-error` types below mirror
//! `theater:simple/runtime` exactly so the interface subset-hash matches the
//! host; get them wrong and the actor compiles but fails to instantiate.

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

pack_types! {
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

    variant runtime-error {
        permission-denied(string),
        runtime-unavailable,
        actor-not-found(string),
        invalid-argument(string),
        spawn-failed(spawn-failure),
        internal(string),
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/runtime {
            spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, runtime-error>,
        }
        theater:simple/lifecycle {
            monitor: func(subject: string) -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(config: value) -> result<_, string>,
        theater:simple/lifecycle-handlers.handle-actor-event: func(subject: string, event-type: string, data: list<u8>) -> result<_, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/runtime", name = "spawn")]
fn runtime_spawn(
    manifest: String,
    init_state: Option<Value>,
    wasm_bytes: Option<Vec<u8>>,
) -> Value;

#[import(module = "theater:simple/lifecycle", name = "monitor")]
fn lifecycle_monitor(subject: String) -> Result<(), String>;

/// `result<_, string>::ok(())` — no state to return.
fn ok_unit() -> Value {
    let unit = Value::Tuple(Vec::new());
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
    // Spawn the sibling `hello` example as our child. (Run this from the
    // `examples/` dir, with hello built, for the path to resolve.) `None` for
    // init-state and wasm-bytes — the packr-guest macro marshals Rust `Option`.
    match runtime_spawn(String::from("hello/manifest.toml"), None, None) {
        Value::Variant { tag: 0, payload, .. } => match payload.into_iter().next() {
            Some(Value::String(id)) => {
                log(format!("supervisor: spawned child {}", id));
                // Attach a monitor so the child's terminal event is delivered
                // to our handle-actor-event (spawn no longer auto-monitors).
                if let Err(e) = lifecycle_monitor(id) {
                    log(format!("supervisor: monitor failed: {}", e));
                }
            }
            _ => log(String::from("supervisor: spawned child (id unavailable)")),
        },
        _ => log(String::from("supervisor: spawn failed")),
    }
    ok_unit()
}

/// The lifecycle handler calls this when a monitored child terminates. The
/// params are (child-id, event-type, terminal-payload-bytes); we just log it.
#[export(name = "theater:simple/lifecycle-handlers.handle-actor-event")]
fn handle_actor_event(input: Value) -> Value {
    let id = match &input {
        Value::Tuple(items) if !items.is_empty() => match &items[0] {
            Value::String(s) => s.clone(),
            _ => String::from("<unknown>"),
        },
        _ => String::from("<unknown>"),
    };
    log(format!("supervisor: my child {} terminated", id));
    ok_unit()
}
