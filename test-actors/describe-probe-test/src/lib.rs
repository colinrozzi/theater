//! Guest actor for the `rpc.describe` guest-level test. It imports
//! `theater:simple/rpc.describe` and, from inside an exported call (so it holds
//! its own execution lock), describes each target id it is given, returning a
//! `list<bool>` of "describe returned ok" per target.
//!
//! This exercises the REAL host result encoding end-to-end (a guest decoding
//! describe's `result<value,string>`), the unknown-actor error path, and —
//! critically — SELF-describe: a target equal to this actor's own id must NOT
//! deadlock (it would, if describe took the execution lock instead of the
//! instantiation-time cache).

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

pack_types! {
    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/rpc {
            describe: func(actor-id: string) -> value,
        }
    }
    exports {
        theater:simple/actor.init: func(config: value) -> result<_, string>,
        // Describe each target id; return list<bool> of (describe ok) per target.
        theater:test/describe-probe.probe: func(targets: list<string>) -> value,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/rpc", name = "describe")]
fn rpc_describe(actor_id: String) -> Value;

fn ok_unit() -> Value {
    let unit = Value::Tuple(vec![]);
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
    log(String::from("describe-probe: init"));
    ok_unit()
}

/// Describe each target; `true` iff describe returned `result::ok`.
#[export(name = "theater:test/describe-probe.probe")]
fn probe(targets: Vec<String>) -> Value {
    let mut flags: Vec<Value> = Vec::new();
    for id in targets {
        let result = rpc_describe(id.clone());
        // describe returns result<value, string>: ok => describe succeeded.
        let ok = matches!(
            result,
            Value::Result {
                value: Ok(_),
                ..
            }
        );
        log(alloc::format!("describe-probe: {} -> ok={}", id, ok));
        flags.push(Value::Bool(ok));
    }
    Value::List {
        elem_type: ValueType::Bool,
        items: flags,
    }
}
