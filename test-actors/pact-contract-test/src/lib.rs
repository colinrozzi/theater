//! Test actor with contract defined in an external .pact file.
//!
//! A simple todo list actor demonstrating:
//! - External type definitions via pack_types!(file = "...")
//! - Record types with list fields
//! - In-module state that evolves across calls
//!
//! State lives inside the module now (`docs/in-module-state.md`): the todo list
//! `actor-state` is held in a `StateCell` (a hand-built pack `Value`, so it uses
//! the cell directly rather than `#[derive(State)]`), set in `init` and mutated
//! in place. A manual `get-state` export keeps it inspectable.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, Value, ValueType};
use theater_guest::StateCell;

packr_guest::setup_guest!();

// Load type definitions from external .pact file
pack_types!(file = "types.pact");

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

/// The actor's `actor-state`, held inside the module as a hand-built Value.
static STATE: StateCell<Value> = StateCell::new();

fn make_todo(id: u32, title: String, done: bool) -> Value {
    Value::Record {
        type_name: String::from("todo-item"),
        fields: vec![
            (String::from("id"), Value::U32(id)),
            (String::from("title"), Value::String(title)),
            (String::from("done"), Value::Bool(done)),
        ],
    }
}

fn make_state(items: Vec<Value>, next_id: u32) -> Value {
    Value::Record {
        type_name: String::from("actor-state"),
        fields: vec![
            (String::from("items"), Value::List {
                elem_type: ValueType::Record(String::from("todo-item")),
                items,
            }),
            (String::from("next-id"), Value::U32(next_id)),
        ],
    }
}

fn extract_state(state: &Value) -> (Vec<Value>, u32) {
    match state {
        Value::Record { fields, .. } => {
            let mut items = Vec::new();
            let mut next_id = 0u32;
            for (name, val) in fields {
                match name.as_str() {
                    "items" => {
                        if let Value::List { items: list, .. } = val {
                            items = list.clone();
                        }
                    }
                    "next-id" => {
                        if let Value::U32(n) = val {
                            next_id = *n;
                        }
                    }
                    _ => {}
                }
            }
            (items, next_id)
        }
        _ => (Vec::new(), 0),
    }
}

fn ok_result(value: Value) -> Value {
    Value::Result {
        ok_type: value.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(value)),
    }
}

/// `result<_, string>::ok(())` — nothing to return but success.
fn ok_unit() -> Value {
    let unit = Value::Tuple(vec![]);
    Value::Result {
        ok_type: unit.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(unit)),
    }
}

fn err_result(msg: &str) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(vec![]),
        err_type: ValueType::String,
        value: Err(Box::new(Value::String(String::from(msg)))),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
    log(String::from("[todo] init"));
    STATE.set(make_state(Vec::new(), 1));
    ok_unit()
}

/// Manual `get-state` export: serialize the current in-module state.
#[export(name = "theater:simple/actor.get-state")]
fn get_state() -> Value {
    STATE.with(|s| s.clone())
}

#[export(name = "theater:todo/actions.add")]
fn add(title: Value) -> Value {
    let (mut items, next_id) = STATE.with(|s| extract_state(s));
    let title_str = match &title {
        Value::String(s) => s.clone(),
        _ => return err_result("Expected string title"),
    };

    log(format!("[todo] add: {} (id={})", title_str, next_id));

    let new_todo = make_todo(next_id, title_str, false);
    items.push(new_todo.clone());

    STATE.set(make_state(items, next_id + 1));
    ok_result(new_todo)
}

#[export(name = "theater:todo/actions.toggle")]
fn toggle(id_val: Value) -> Value {
    let target_id = match &id_val {
        Value::U32(n) => *n,
        _ => return err_result("Expected u32 id"),
    };

    let (items, next_id) = STATE.with(|s| extract_state(s));

    log(format!("[todo] toggle id={}", target_id));

    let new_items: Vec<Value> = items.into_iter().map(|item| {
        if let Value::Record { type_name, fields } = &item {
            let mut is_target = false;
            for (name, val) in fields {
                if name == "id" {
                    if let Value::U32(id) = val {
                        if *id == target_id {
                            is_target = true;
                        }
                    }
                }
            }
            if is_target {
                let new_fields: Vec<(String, Value)> = fields.iter().map(|(name, val)| {
                    if name == "done" {
                        if let Value::Bool(b) = val {
                            return (name.clone(), Value::Bool(!b));
                        }
                    }
                    (name.clone(), val.clone())
                }).collect();
                Value::Record {
                    type_name: type_name.clone(),
                    fields: new_fields,
                }
            } else {
                item.clone()
            }
        } else {
            item
        }
    }).collect();

    STATE.set(make_state(new_items, next_id));
    ok_unit()
}

#[export(name = "theater:todo/actions.list")]
fn list() -> Value {
    let (items, _next_id) = STATE.with(|s| extract_state(s));

    log(format!("[todo] list: {} items", items.len()));

    let items_list = Value::List {
        elem_type: ValueType::Record(String::from("todo-item")),
        items,
    };

    ok_result(items_list)
}
