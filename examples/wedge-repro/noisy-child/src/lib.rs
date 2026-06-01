//! Wedge-reproduction noisy child.
//!
//! On init, emits LOG_BURSTS messages via theater:simple/runtime.log. Each
//! log call becomes a chain event on this actor's chain, which the
//! supervising parent receives via handle-child-event and records on its own
//! chain — the amplification mechanism documented in theater-dev's wedge
//! diagnosis.
//!
//! Burst size is tuned to be enough to saturate the runtime command channel
//! on the supervisor side under typical configurations. See README.md.

#![no_std]

extern crate alloc;

use alloc::string::String;
use packr_guest::{export, import, pack_types, Value};

packr_guest::setup_guest!();

/// How many log events to emit on init. Tune up if 100k doesn't trip the
/// wedge in your environment; tune down to find the threshold.
const LOG_BURSTS: u32 = 100_000;

pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<tuple<bool, _>, string>,
    }
}

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[export(name = "theater:simple/actor.init")]
fn init(_state: Value) -> Result<(bool, ()), String> {
    log(String::from("[noisy-child] init — emitting burst"));
    // Allocate the message once outside the loop so we measure theater's
    // recording cost, not the actor's per-event allocation.
    let msg = String::from("[noisy-child] event");
    for _ in 0..LOG_BURSTS {
        log(msg.clone());
    }
    log(String::from("[noisy-child] burst complete, idling"));
    Ok((true, ()))
}
