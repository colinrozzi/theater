//! `hello` — the smallest possible Theater actor.
//!
//! On startup the runtime calls `init`; this actor logs a greeting and returns
//! its (trivial) initial state. It demonstrates the two things every actor has:
//! the `theater:simple/actor.init` export and an import from the host
//! (`theater:simple/self.log`).

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

pack_types! {
    record hello-state {
        greeted: bool,
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<hello-state, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[export(name = "theater:simple/actor.init")]
fn init(_input: Value) -> Value {
    log(String::from("hello: 👋 hello from a Theater actor!"));

    let state = Value::Record {
        type_name: String::from("hello-state"),
        fields: vec![(String::from("greeted"), Value::Bool(true))],
    };
    Value::Result {
        ok_type: state.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(state)),
    }
}
