//! RPC Caller Actor
//!
//! Demonstrates using the RPC handler to call functions on other actors.
//!
//! This actor:
//! - Imports `theater:simple/rpc.{call, implements, exports}` for RPC
//! - Calls math functions on the calculator actor
//! - Logs the results

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use pack_guest::{export, import, Value, ValueType};

pack_guest::setup_guest!();

pack_guest::pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/rpc {
            call: func(actor_id: string, function: string, params: value, options: value) -> value,
            implements: func(actor_id: string, interface: string) -> value,
            exports: func(actor_id: string) -> value,
        }
    }
    exports {
        theater:simple/actor.init: func(input: value) -> value,
        my:caller.run-demo: func(input: value) -> value,
    }
}

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

/// Call a function on another actor
/// Returns a Value that contains the result (ok/err variant)
#[import(module = "theater:simple/rpc", name = "call")]
fn rpc_call(
    actor_id: String,
    function: String,
    params: Value,
    options: Value,  // option<call-options> - pass Value::Option { value: None } for no options
) -> Value;

/// Check if an actor implements an interface
/// Returns a Value containing result<bool, string>
#[import(module = "theater:simple/rpc", name = "implements")]
fn rpc_implements(actor_id: String, interface: String) -> Value;

/// Get list of interfaces exported by an actor
/// Returns a Value containing result<list<string>, string>
#[import(module = "theater:simple/rpc", name = "exports")]
fn rpc_exports(actor_id: String) -> Value;

// ============================================================================
// Exports
// ============================================================================

/// Initialize the actor
#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("rpc-caller: initialized"));
    ok_state(state)
}

/// Run the demo: call calculator functions via RPC
/// run-demo(state, calculator_id: string) -> result<tuple<state, string>, string>
#[export(name = "my:caller.run-demo")]
fn run_demo(input: Value) -> Value {
    // Input is (state, params) where params = calculator_id: string
    let (state, calculator_id) = match &input {
        Value::Tuple(items) if items.len() >= 2 => {
            let state = items[0].clone();
            let calc_id = match &items[1] {
                Value::Tuple(p) if !p.is_empty() => match &p[0] {
                    Value::String(s) => s.clone(),
                    _ => return err_result("Expected string calculator ID"),
                },
                Value::String(s) => s.clone(),
                _ => return err_result("Expected calculator ID"),
            };
            (state, calc_id)
        }
        _ => return err_result("Invalid input format"),
    };

    log(String::from("rpc-caller: starting demo"));

    // Helper: create None option value for call options
    let none_options = Value::Option {
        inner_type: ValueType::Bool,  // Type doesn't matter for None
        value: None,
    };

    // First, check what interfaces the calculator exports
    log(String::from("rpc-caller: checking calculator exports..."));
    let exports_result = rpc_exports(calculator_id.clone());
    log(format_log("  exports result: ", &format_value(&exports_result)));

    // Check if calculator implements our expected interface
    log(String::from("rpc-caller: checking my:calculator interface..."));
    let implements_result = rpc_implements(calculator_id.clone(), String::from("my:calculator"));
    log(format_log("  implements result: ", &format_value(&implements_result)));

    // Call add(10, 5)
    log(String::from("rpc-caller: calling add(10, 5)..."));
    let add_params = Value::Tuple(vec![Value::S32(10), Value::S32(5)]);
    let add_result = rpc_call(
        calculator_id.clone(),
        String::from("my:calculator.add"),
        add_params,
        none_options.clone(),
    );
    log(format_log("  add result: ", &format_value(&add_result)));

    // Call subtract(10, 3)
    log(String::from("rpc-caller: calling subtract(10, 3)..."));
    let sub_params = Value::Tuple(vec![Value::S32(10), Value::S32(3)]);
    let sub_result = rpc_call(
        calculator_id.clone(),
        String::from("my:calculator.subtract"),
        sub_params,
        none_options.clone(),
    );
    log(format_log("  subtract result: ", &format_value(&sub_result)));

    // Call multiply(7, 6)
    log(String::from("rpc-caller: calling multiply(7, 6)..."));
    let mul_params = Value::Tuple(vec![Value::S32(7), Value::S32(6)]);
    let mul_result = rpc_call(
        calculator_id.clone(),
        String::from("my:calculator.multiply"),
        mul_params,
        none_options.clone(),
    );
    log(format_log("  multiply result: ", &format_value(&mul_result)));

    // Call divide(20, 4)
    log(String::from("rpc-caller: calling divide(20, 4)..."));
    let div_params = Value::Tuple(vec![Value::S32(20), Value::S32(4)]);
    let div_result = rpc_call(
        calculator_id.clone(),
        String::from("my:calculator.divide"),
        div_params,
        none_options.clone(),
    );
    log(format_log("  divide result: ", &format_value(&div_result)));

    // Call divide(10, 0) - should return error in the inner result
    log(String::from("rpc-caller: calling divide(10, 0)..."));
    let div_zero_params = Value::Tuple(vec![Value::S32(10), Value::S32(0)]);
    let div_zero_result = rpc_call(
        calculator_id.clone(),
        String::from("my:calculator.divide"),
        div_zero_params,
        none_options.clone(),
    );
    log(format_log("  divide by zero result: ", &format_value(&div_zero_result)));

    log(String::from("rpc-caller: demo complete!"));

    // Return success with summary
    ok_result_with_state(state, Value::String(String::from("Demo completed successfully")))
}

// ============================================================================
// Helpers
// ============================================================================

fn format_log(prefix: &str, msg: &str) -> String {
    let mut s = String::from(prefix);
    s.push_str(msg);
    s
}

fn format_value(v: &Value) -> String {
    match v {
        Value::S32(n) => {
            let mut s = String::from("i32:");
            if *n < 0 {
                s.push('-');
            }
            let abs = if *n < 0 { -n } else { *n } as u32;
            format_u32(&mut s, abs);
            s
        }
        Value::Bool(b) => String::from(if *b { "true" } else { "false" }),
        Value::String(s) => s.clone(),
        Value::Variant { case_name, payload, .. } => {
            let mut s = String::from("variant:");
            s.push_str(case_name);
            if !payload.is_empty() {
                s.push('(');
                s.push_str(&format_value(&payload[0]));
                s.push(')');
            }
            s
        }
        Value::Tuple(items) => {
            let mut s = String::from("tuple(");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&format_value(item));
            }
            s.push(')');
            s
        }
        Value::List { items, .. } => {
            let mut s = String::from("list[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&format_value(item));
            }
            s.push(']');
            s
        }
        Value::Option { value: Some(inner), .. } => {
            let mut s = String::from("some(");
            s.push_str(&format_value(inner));
            s.push(')');
            s
        }
        Value::Option { value: None, .. } => String::from("none"),
        _ => String::from("<value>"),
    }
}

fn format_u32(s: &mut String, mut n: u32) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        digits[i] = (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push((b'0' + digits[i]) as char);
    }
}

fn err_result(msg: &str) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("err"),
        tag: 1,
        payload: vec![Value::String(String::from(msg))],
    }
}

fn ok_state(state: Value) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: vec![Value::Tuple(vec![state])],
    }
}

fn ok_result_with_state(state: Value, result: Value) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("ok"),
        tag: 0,
        payload: vec![Value::Tuple(vec![state, result])],
    }
}
