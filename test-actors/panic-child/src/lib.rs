//! A minimal actor whose only job is to panic on demand, in a POST-init call.
//!
//! `init` just logs and returns ok — the actor comes up healthy. The exported
//! `boom` panics: with `packr-guest` 0.24.1 the guest panic handler turns that
//! into a wasm **trap**, which the runtime reports as a `Failed` termination.
//! Before 0.24.1 the panic handler spun (`loop {}`) SILENTLY — no trap, no
//! `Failed`, so a supervisor/monitor could never catch a steady-state crash.
//! This actor is the subject in `panic_yields_failed_test`, which locks that
//! path so it can't silently regress.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

// Set up allocator and panic handler (the 0.24.1 handler = trap).
packr_guest::setup_guest!();

// Embed interface metadata.
pack_types! {
    imports {
        theater:simple/self {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(config: value) -> result<_, string>,
        // The trap trigger: a post-init callback that panics.
        theater:simple/panic-child.boom: func() -> result<_, string>,
    }
}

// Import the log function from the host.
#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

/// `result<_, string>::ok(())` — nothing to return but success.
fn ok_unit() -> Value {
    let unit = Value::Tuple(vec![]);
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}

/// Initialize the actor — come up healthy so the panic is a *steady-state* crash.
#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
    log(String::from("panic-child: init called"));
    ok_unit()
}

/// Panic on demand. `packr-guest` 0.24.1 turns this into a wasm trap → the
/// runtime reports `Failed`. There is no return; `panic!` diverges.
#[export(name = "theater:simple/panic-child.boom")]
fn boom() -> Value {
    log(String::from("panic-child: boom called, about to panic"));
    panic!("boom")
}
