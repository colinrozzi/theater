//! A simple test actor for Pack runtime integration with Theater.
//!
//! This actor:
//! 1. Exports `init` function for the theater:simple/actor interface
//! 2. Imports `log` function from theater:simple/runtime interface
//! 3. Demonstrates typed state — init receives empty state, returns a record

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

// Set up allocator and panic handler
pack_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<value, string>,
    }
}

// Import the log function from the host
#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

/// The init function for theater:simple/actor interface.
#[export(name = "theater:simple/actor.init")]
fn init(_input: Value) -> Value {
    log(String::from("Composite test actor: init called!"));
    log(String::from("Composite test actor: init completed successfully!"));

    // Return a typed state record
    let state = Value::Record {
        type_name: String::from("composite-state"),
        fields: vec![
            (String::from("initialized"), Value::Bool(true)),
        ],
    };

    Value::Result {
        ok_type: state.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(state)),
    }
}
