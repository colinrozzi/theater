//! `counter` — state + inter-actor messaging via `theater:simple/message-server`.
//!
//! The actor keeps a running count in its state and bumps it every time it
//! receives a message. It shows the message-server shape:
//! - `register()` (called from `init`) subscribes the actor to its mailbox,
//! - `handle-send(state, (from, msg))` is invoked for each one-way message and
//!   returns the *new* state.
//!
//! Message-server actors carry opaque `option<list<u8>>` state; here that's the
//! count rendered as a decimal string.

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
    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-send: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/message-server-host", name = "register")]
fn register() -> Result<(), String>;

/// State is `option<list<u8>>` holding the count as a decimal string.
fn state_with_count(count: u64) -> Value {
    let bytes = format!("{}", count);
    Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: Some(Box::new(Value::List {
            elem_type: ValueType::U8,
            items: bytes.bytes().map(Value::U8).collect(),
        })),
    }
}

fn count_from_state(state: &Value) -> u64 {
    if let Value::Option {
        value: Some(inner), ..
    } = state
    {
        if let Value::List { items, .. } = inner.as_ref() {
            let bytes: Vec<u8> = items
                .iter()
                .filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None })
                .collect();
            if let Ok(s) = String::from_utf8(bytes) {
                return s.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// Both exports return `result<tuple<new-state>, string>`; wrap a state value.
fn ok_state(state: Value) -> Value {
    let tuple = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: tuple.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(tuple)),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(_input: Value) -> Value {
    // Subscribe to our mailbox so `handle-send` starts firing.
    let _ = register();
    log(String::from("counter: initialized at 0"));
    ok_state(state_with_count(0))
}

#[export(name = "theater:simple/message-server-client.handle-send")]
fn handle_send(input: Value) -> Value {
    // Host flattens to Tuple[state, from, msg]; we only need the state here.
    let state = match input {
        Value::Tuple(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    };
    let next = count_from_state(&state) + 1;
    log(format!("counter: bumped to {}", next));
    ok_state(state_with_count(next))
}
