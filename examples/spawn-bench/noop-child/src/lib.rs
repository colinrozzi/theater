//! Minimal child for spawn-bench: returns from init immediately.
//!
//! The point is to keep the actor's contribution to spawn cost as close to
//! zero as possible so the measured per-phase numbers reflect theater's
//! spawn pipeline, not the actor's init work.

#![no_std]

extern crate alloc;

use alloc::string::String;
use packr_guest::{export, pack_types, Value};

packr_guest::setup_guest!();

pack_types! {
    exports {
        theater:simple/actor.init: func(state: value) -> result<tuple<bool, _>, string>,
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(_state: Value) -> Result<(bool, ()), String> {
    Ok((true, ()))
}
