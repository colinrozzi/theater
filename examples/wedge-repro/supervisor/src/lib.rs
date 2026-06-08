//! Wedge-reproduction supervisor.
//!
//! Spawns the noisy-child actor (via `theater:simple/supervisor.spawn`) and
//! registers `handle-child-event` so theater records every child chain event
//! as a `wasm-call` on this actor's chain. That recording path is the
//! amplification mechanism — each child event becomes a parent chain entry
//! with the child event's payload embedded.
//!
//! Under normal use this is fine; under the burst conditions the noisy-child
//! produces (LOG_BURSTS log events at init), the runtime command channel
//! saturates and the wedge fingerprint appears in theater's stderr:
//!
//!     WARN  Failed to send event notification: no available capacity
//!
//! followed eventually by silent process exit.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value};

packr_guest::setup_guest!();

/// Where to find the noisy-child manifest. Configurable via init state so
/// the integration test / repro README can vary paths without rebuilding.
/// Default assumes theater was invoked from the wedge-repro directory.
const DEFAULT_CHILD_MANIFEST: &str = "./noisy-child/manifest.toml";

pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/supervisor {
            spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, string>,
            subscribe-to-child: func(child-id: string) -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<tuple<bool, _>, string>,
        theater:simple/supervisor-handlers.handle-child-event: func(state: value, child-id: string, event-type: string, event-data: list<u8>) -> result<value, string>,
        theater:simple/supervisor-handlers.handle-child-error: func(state: value, child-id: string, error: value) -> result<value, string>,
        theater:simple/supervisor-handlers.handle-child-exit: func(state: value, child-id: string, result: value) -> result<value, string>,
        theater:simple/supervisor-handlers.handle-child-external-stop: func(state: value, child-id: string) -> result<value, string>,
    }
}

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/supervisor", name = "spawn")]
fn supervisor_spawn(
    manifest: String,
    init_state: Option<Value>,
    wasm_bytes: Option<Vec<u8>>,
) -> Result<String, String>;

#[import(module = "theater:simple/supervisor", name = "subscribe-to-child")]
fn supervisor_subscribe_to_child(child_id: String) -> Result<(), String>;

#[export(name = "theater:simple/actor.init")]
fn init(state: Value) -> Result<(bool, ()), String> {
    log(String::from("[wedge-supervisor] init — spawning noisy-child"));
    let manifest_path = String::from(DEFAULT_CHILD_MANIFEST);
    match supervisor_spawn(manifest_path, None, None) {
        Ok(child_id) => {
            // Opt in to the full firehose — the wedge repro's whole
            // point is that `handle-child-event` fires for every child
            // event so theater records the amplification on this actor's
            // chain. Post-PR opt-in default, the parent must subscribe
            // explicitly for that path to engage.
            if let Err(e) = supervisor_subscribe_to_child(child_id.clone()) {
                log(format!(
                    "[wedge-supervisor] subscribe-to-child failed: {}",
                    e
                ));
                return Err(e);
            }
            log(format!(
                "[wedge-supervisor] spawned noisy-child {} — waiting for burst events to amplify",
                child_id
            ));
        }
        Err(e) => {
            log(format!(
                "[wedge-supervisor] spawn failed: {}",
                e
            ));
            return Err(e);
        }
    }
    let _ = state;
    Ok((true, ()))
}

#[export(name = "theater:simple/supervisor-handlers.handle-child-event")]
fn handle_child_event(
    state: Value,
    _child_id: String,
    _event_type: String,
    _event_data: Vec<u8>,
) -> Result<Value, String> {
    // No-op body. The amplification we're studying is in the THEATER RUNTIME
    // recording this very call — the supervisor doesn't need to do anything
    // with the event for the wedge to manifest. (sentinel in prod also has a
    // near-no-op body when bombarded — its rate-limit check is cheap.)
    Ok(state)
}

#[export(name = "theater:simple/supervisor-handlers.handle-child-error")]
fn handle_child_error(state: Value, _child_id: String, _error: Value) -> Result<Value, String> {
    log(String::from("[wedge-supervisor] child errored"));
    Ok(state)
}

#[export(name = "theater:simple/supervisor-handlers.handle-child-exit")]
fn handle_child_exit(state: Value, _child_id: String, _result: Value) -> Result<Value, String> {
    log(String::from("[wedge-supervisor] child exited"));
    Ok(state)
}

#[export(name = "theater:simple/supervisor-handlers.handle-child-external-stop")]
fn handle_child_external_stop(state: Value, _child_id: String) -> Result<Value, String> {
    log(String::from("[wedge-supervisor] child externally stopped"));
    Ok(state)
}
