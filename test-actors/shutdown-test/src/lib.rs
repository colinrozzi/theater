//! Minimal actor for shutdown timing tests.
//!
//! This actor is as simple as possible - just a runtime handler and init.
//! Used to test that actors shut down quickly without the 10-second timeout.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

pack_guest::setup_guest!();

pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>)
            -> result<tuple<option<list<u8>>>, string>,
    }
}

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[export(name = "theater:simple/actor.init")]
fn init(_input: Value) -> Value {
    log(String::from("[shutdown-test] Init called"));

    // Return empty state
    let state = Value::Tuple(vec![
        Value::Option {
            inner_type: ValueType::List(Box::new(ValueType::U8)),
            value: None,
        },
    ]);

    Value::Result {
        ok_type: state.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(state)),
    }
}
