//! Monitor test actor — exercises `theater:simple/lifecycle` end-to-end.
//!
//! On init it is given a subject actor id (in its init state) and calls
//! `monitor(subject)`. It exports `handle-lifecycle-event`, and records the
//! event-type of each delivered event into its own state, so a test can spawn
//! this actor, stop the subject, and observe that `"terminated"` was delivered.

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
    record monitor-state {
        received: string,
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/lifecycle {
            monitor: func(subject: string) -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<monitor-state, string>,
        theater:simple/lifecycle-handlers.handle-lifecycle-event: func(state: monitor-state, params: tuple<string, string, list<u8>>) -> result<tuple<monitor-state>, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/lifecycle", name = "monitor")]
fn monitor(subject: String) -> Result<(), String>;

fn make_state(received: &str) -> Value {
    Value::Record {
        type_name: String::from("monitor-state"),
        fields: vec![(
            String::from("received"),
            Value::String(String::from(received)),
        )],
    }
}

fn ok_result(value: Value) -> Value {
    Value::Result {
        ok_type: value.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(value)),
    }
}

/// Pull the subject id (a utf-8 string in `option<list<u8>>`) out of init state.
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
    // init is called as a 1-tuple wrapping the init state.
    let init_state = match input {
        Value::Tuple(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    };
    match subject_from_init(&init_state) {
        Some(subject) => {
            log(format!("monitor-test: monitoring {}", subject));
            let _ = monitor(subject);
            ok_result(make_state("init"))
        }
        None => {
            log(String::from("monitor-test: no subject id in init state"));
            ok_result(make_state("no-subject"))
        }
    }
}

#[export(name = "theater:simple/lifecycle-handlers.handle-lifecycle-event")]
fn handle_lifecycle_event(input: Value) -> Value {
    // The host flattens the call to Tuple[state, ..params], so the args are:
    // [0]=state, [1]=subject, [2]=event-type, [3]=data.
    let event_type = match &input {
        Value::Tuple(items) if items.len() >= 3 => match &items[2] {
            Value::String(s) => s.clone(),
            _ => String::from("?"),
        },
        _ => String::from("?"),
    };
    log(format!("monitor-test: received lifecycle event {}", event_type));
    // New state records the event-type we just saw.
    ok_result(Value::Tuple(vec![make_state(&event_type)]))
}
