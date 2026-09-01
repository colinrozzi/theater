//! `link` — fate-sharing between actors via `theater:simple/lifecycle`.
//!
//! This actor is handed a *subject* actor id in its init state and calls
//! `link(subject)`. A link is fate-sharing: when the subject terminates for any
//! reason, the runtime stops **this** actor too (termination cause
//! `PeerKilled`). No callback is needed — linking is the whole behavior.
//!
//! (Its sibling primitive is `monitor`, which instead *delivers* the subject's
//! lifecycle events to `handle-lifecycle-event` without sharing fate.)

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

/// Init state is `option<list<u8>>` carrying the subject id as UTF-8 bytes.
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

fn ok_state(subject: &str) -> Value {
    let state = Value::Record {
        type_name: String::from("link-state"),
        fields: vec![(
            String::from("subject"),
            Value::String(String::from(subject)),
        )],
    };
    Value::Result {
        ok_type: state.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(state)),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    // The runtime flattens the init call to Tuple[state]; unwrap the state arg.
    let init_state = match input {
        Value::Tuple(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    };
    match subject_from_init(&init_state) {
        Some(subject) => {
            log(format!("link: linking my fate to {}", subject));
            let _ = link(subject.clone());
            ok_state(&subject)
        }
        None => {
            log(String::from("link: no subject id in init state; nothing to link"));
            ok_state("")
        }
    }
}
