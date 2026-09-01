//! `link` — fate-sharing between actors via `theater:simple/lifecycle`.
//!
//! This actor is handed a *subject* actor id in its init config and calls
//! `link(subject)`. A link is fate-sharing: when the subject terminates for any
//! reason, the runtime stops **this** actor too (termination cause
//! `PeerKilled`). No callback is needed — linking is the whole behavior. State
//! lives inside the module now (`docs/in-module-state.md`); this actor holds
//! none — it only acts on init.
//!
//! (Its sibling primitive is `monitor`, which instead *delivers* the subject's
//! lifecycle events to `handle-lifecycle-event` without sharing fate.)

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

pack_types! {
    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/lifecycle {
            link: func(subject: string) -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(config: value) -> result<_, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/lifecycle", name = "link")]
fn link(subject: String) -> Result<(), String>;

/// The init config carries the subject id as UTF-8 bytes in `option<list<u8>>`.
fn subject_from_config(config: &Value) -> Option<String> {
    match config {
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

/// `result<_, string>::ok(())` — no state to return.
fn ok_unit() -> Value {
    let unit = Value::Tuple(Vec::new());
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(config: Value) -> Value {
    // The subject id arrives as the manifest config; unwrap any tuple wrapping.
    let config = match config {
        Value::Tuple(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    };
    match subject_from_config(&config) {
        Some(subject) => {
            log(format!("link: linking my fate to {}", subject));
            let _ = link(subject);
        }
        None => {
            log(String::from("link: no subject id in config; nothing to link"));
        }
    }
    ok_unit()
}
