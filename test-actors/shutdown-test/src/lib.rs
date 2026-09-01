//! Minimal actor for shutdown timing tests.
//!
//! This actor is as simple as possible - just a runtime handler and init.
//! Used to test that actors shut down quickly without the 10-second timeout.
//! State lives inside the module now (`docs/in-module-state.md`); this actor
//! holds none.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

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

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
    log(String::from("[shutdown-test] Init called"));

    // result<_, string>::ok(()) — no state to return.
    let unit = Value::Tuple(Vec::new());
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}
