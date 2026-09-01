//! A simple test actor for Pack runtime integration with Theater.
//!
//! This actor:
//! 1. Exports `init` function for the theater:simple/actor interface
//! 2. Imports `log` function from theater:simple/self interface
//!
//! State lives inside the module now (`docs/in-module-state.md`); this actor
//! holds none — it just logs on init.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

// Set up allocator and panic handler
packr_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types! {
    imports {
        theater:simple/self {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(config: value) -> result<_, string>,
    }
}

// Import the log function from the host
#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

/// The init function for theater:simple/actor interface.
#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
    log(String::from("Composite test actor: init called!"));
    log(String::from("Composite test actor: init completed successfully!"));

    // result<_, string>::ok(()) — no state to return.
    let unit = Value::Tuple(Vec::new());
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}
