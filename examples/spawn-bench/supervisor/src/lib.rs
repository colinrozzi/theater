//! Spawn-bench supervisor.
//!
//! Fires `SPAWN_COUNT` sequential `supervisor.spawn(target, none, none)`
//! calls and lets the host's instrumentation (info! lines tagged
//! `phase = supervisor.* | runtime.*`) carry the timing data out on stderr.
//!
//! Why sequential: matches the per-conn-child shape — one parent acceptor
//! spawning per-request. The runtime command loop is the serialization
//! point we're measuring. Multi-parent parallel load is a follow-up.
//!
//! Tuning: edit `SPAWN_COUNT` and `TARGET_MANIFEST` and rebuild. The
//! aggregate p50/p95/p99 comes from parsing theater's stderr (see
//! ../README.md).

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use packr_guest::{export, import, pack_types, Value};

packr_guest::setup_guest!();

const TARGET_MANIFEST: &str = "./noop-child/manifest.toml";
const SPAWN_COUNT: u32 = 50;

pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
            shutdown: func(data: option<list<u8>>) -> result<_, string>,
        }
        theater:simple/supervisor {
            spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<tuple<bool, _>, string>,
    }
}

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/runtime", name = "shutdown")]
fn runtime_shutdown(data: Option<alloc::vec::Vec<u8>>) -> Result<(), String>;

#[import(module = "theater:simple/supervisor", name = "spawn")]
fn supervisor_spawn(
    manifest: String,
    init_state: Option<Value>,
    wasm_bytes: Option<alloc::vec::Vec<u8>>,
) -> Result<String, String>;

#[export(name = "theater:simple/actor.init")]
fn init(_state: Value) -> Result<(bool, ()), String> {
    log(format!(
        "[spawn-bench] starting {} sequential spawns of {}",
        SPAWN_COUNT, TARGET_MANIFEST
    ));

    let mut ok: u32 = 0;
    let mut err: u32 = 0;
    for i in 0..SPAWN_COUNT {
        match supervisor_spawn(String::from(TARGET_MANIFEST), None, None) {
            Ok(_id) => ok += 1,
            Err(e) => {
                err += 1;
                log(format!("[spawn-bench] spawn {} failed: {}", i, e));
            }
        }
    }
    log(format!(
        "[spawn-bench] done — ok={} err={} (see runtime.* / supervisor.* phase lines)",
        ok, err
    ));
    // Self-terminate so the host runtime exits cleanly and its stderr
    // (with all the phase lines) flushes — otherwise the bench has to be
    // SIGTERM'd by `timeout`, which truncates the capture.
    let _ = runtime_shutdown(None);
    Ok((true, ()))
}
