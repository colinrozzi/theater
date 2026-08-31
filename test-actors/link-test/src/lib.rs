//! Link test actor — exercises `theater:simple/lifecycle.link` end-to-end.
//!
//! On init it is handed a subject actor id (in its init state) and calls
//! `link(subject)`. Fate-linking needs no callback: when the subject terminates,
//! the runtime stops *this* actor (cause `PeerKilled`). A test spawns this actor,
//! stops the subject, and observes this actor disappear.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

pack_types! {
    record link-state {
        subject: string,
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/lifecycle {
            link: func(subject: string) -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<link-state, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/lifecycle", name = "link")]
fn link(subject: String) -> Result<(), String>;

fn make_state(subject: &str) -> Value {
    Value::Record {
        type_name: String::from("link-state"),
        fields: vec![(String::from("subject"), Value::String(String::from(subject)))],
    }
}

fn ok_result(value: Value) -> Value {
    Value::Result {
        ok_type: value.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(value)),
    }
}

fn subject_from_init(init_state: &Value) -> Option<String> {
    match init_state {
        Value::Option {
            value: Some(inner), ..
        } => match inner.as_ref() {
            Value::List { items, .. } => {
                let bytes: Vec<u8> = items
                    .iter()
                    .filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None })
                    .collect();
                String::from_utf8(bytes).ok()
            }
            _ => None,
        },
        _ => None,
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let init_state = match input {
        Value::Tuple(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    };
    match subject_from_init(&init_state) {
        Some(subject) => {
            log(format!("link-test: linking to {}", subject));
            let _ = link(subject.clone());
            ok_result(make_state(&subject))
        }
        None => {
            log(String::from("link-test: no subject id in init state"));
            ok_result(make_state(""))
        }
    }
}
