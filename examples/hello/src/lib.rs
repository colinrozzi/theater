//! `hello` — the smallest possible Theater actor.
//!
//! On startup the runtime calls `init`, passing the manifest's `initial_state`
//! as the init argument (this actor ignores it). It demonstrates the two things
//! every actor has: the `theater:simple/actor.init` export and an import from the
//! host (`theater:simple/self.log`). State lives inside the module now
//! (`docs/in-module-state.md`) — nothing is threaded through the call, and this
//! stateless actor holds none.

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
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
    log(String::from("hello: 👋 hello from a Theater actor!"));

    // result<_, string>::ok(()) — no state to return.
    let unit = Value::Tuple(alloc::vec::Vec::new());
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}
