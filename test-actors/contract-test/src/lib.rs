//! Test actor for contract enforcement.
//!
//! Exercises rich types: records, variants, nested types.
//! The runtime should validate all inputs and outputs against these declarations.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

pack_guest::setup_guest!();

pack_types! {
    record position {
        x: f64,
        y: f64,
    }

    variant status {
        idle,
        moving(position),
        error(string),
    }

    record actor-state {
        name: string,
        pos: position,
        status: status,
        step-count: u32,
    }

    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<actor-state, string>,
        theater:contract-test/actions.move-to: func(state: actor-state, target: position) -> result<tuple<actor-state, status>, string>,
        theater:contract-test/actions.get-status: func(state: actor-state) -> result<tuple<actor-state, status>, string>,
        theater:contract-test/actions.set-error: func(state: actor-state, msg: string) -> result<tuple<actor-state>, string>,
    }
}

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

fn make_position(x: f64, y: f64) -> Value {
    Value::Record {
        type_name: String::from("position"),
        fields: vec![
            (String::from("x"), Value::F64(x)),
            (String::from("y"), Value::F64(y)),
        ],
    }
}

fn make_status_idle() -> Value {
    Value::Variant {
        type_name: String::from("status"),
        case_name: String::from("idle"),
        tag: 0,
        payload: vec![],
    }
}

fn make_status_moving(pos: Value) -> Value {
    Value::Variant {
        type_name: String::from("status"),
        case_name: String::from("moving"),
        tag: 1,
        payload: vec![pos],
    }
}

fn make_status_error(msg: String) -> Value {
    Value::Variant {
        type_name: String::from("status"),
        case_name: String::from("error"),
        tag: 2,
        payload: vec![Value::String(msg)],
    }
}

fn make_state(name: String, pos: Value, status: Value, step_count: u32) -> Value {
    Value::Record {
        type_name: String::from("actor-state"),
        fields: vec![
            (String::from("name"), Value::String(name)),
            (String::from("pos"), pos),
            (String::from("status"), status),
            (String::from("step-count"), Value::U32(step_count)),
        ],
    }
}

fn extract_state_fields(state: &Value) -> (String, Value, Value, u32) {
    match state {
        Value::Record { fields, .. } => {
            let mut name = String::new();
            let mut pos = make_position(0.0, 0.0);
            let mut status = make_status_idle();
            let mut step_count = 0u32;
            for (fname, val) in fields {
                match fname.as_str() {
                    "name" => if let Value::String(s) = val { name = s.clone(); },
                    "pos" => pos = val.clone(),
                    "status" => status = val.clone(),
                    "step-count" => if let Value::U32(n) = val { step_count = *n; },
                    _ => {}
                }
            }
            (name, pos, status, step_count)
        }
        _ => (String::from("unknown"), make_position(0.0, 0.0), make_status_idle(), 0),
    }
}

fn ok_result(value: Value) -> Value {
    Value::Result {
        ok_type: value.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(value)),
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
fn init(_input: Value) -> Value {
    log(String::from("[contract-test] init"));
    let state = make_state(
        String::from("contract-actor"),
        make_position(0.0, 0.0),
        make_status_idle(),
        0,
    );
    ok_result(state)
}

#[export(name = "theater:contract-test/actions.move-to")]
fn move_to(input: Value) -> Value {
    let (state, target) = match &input {
        Value::Tuple(items) if items.len() >= 2 => (&items[0], &items[1]),
        _ => return err_result("Expected tuple with state and target"),
    };

    let (name, _pos, _status, step_count) = extract_state_fields(state);

    // Extract target position
    let (tx, ty) = match target {
        Value::Record { fields, .. } => {
            let mut x = 0.0f64;
            let mut y = 0.0f64;
            for (fname, val) in fields {
                match fname.as_str() {
                    "x" => if let Value::F64(v) = val { x = *v; },
                    "y" => if let Value::F64(v) = val { y = *v; },
                    _ => {}
                }
            }
            (x, y)
        }
        _ => return err_result("Expected position record"),
    };

    log(format!("[contract-test] moving to ({}, {})", tx, ty));

    let new_pos = make_position(tx, ty);
    let new_status = make_status_moving(new_pos.clone());
    let new_state = make_state(name, new_pos, new_status.clone(), step_count + 1);

    ok_result(Value::Tuple(vec![new_state, new_status]))
}

#[export(name = "theater:contract-test/actions.get-status")]
fn get_status(input: Value) -> Value {
    let state = match &input {
        Value::Tuple(items) if !items.is_empty() => &items[0],
        _ => return err_result("Expected tuple with state"),
    };

    let (_name, _pos, status, _step_count) = extract_state_fields(state);
    log(format!("[contract-test] get-status"));

    ok_result(Value::Tuple(vec![state.clone(), status]))
}

#[export(name = "theater:contract-test/actions.set-error")]
fn set_error(input: Value) -> Value {
    let (state, msg) = match &input {
        Value::Tuple(items) if items.len() >= 2 => (&items[0], &items[1]),
        _ => return err_result("Expected tuple with state and msg"),
    };

    let (name, pos, _status, step_count) = extract_state_fields(state);
    let error_msg = match msg {
        Value::String(s) => s.clone(),
        _ => return err_result("Expected string message"),
    };

    log(format!("[contract-test] set-error: {}", error_msg));

    let new_status = make_status_error(error_msg);
    let new_state = make_state(name, pos, new_status, step_count);

    ok_result(Value::Tuple(vec![new_state]))
}
