//! A simple test actor for Composite runtime integration with Theater.
//!
//! This actor:
//! 1. Exports `init` function for the theater:simple/actor interface
//! 2. Imports `log` function from theater:simple/runtime interface
//! 3. Demonstrates basic state handling

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use composite_guest::{export, Value, encode};

// Set up allocator and panic handler
composite_guest::setup_guest!();

// Import the log function from the host
// In Composite, we need to declare external functions that match the Graph ABI signature
#[link(wasm_import_module = "theater:simple/runtime")]
extern "C" {
    #[link_name = "log"]
    fn host_log(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;
}

/// Call the host's log function with a message
fn log(msg: &str) {
    // Encode the message as a Value::String
    let input = Value::String(String::from(msg));
    let input_bytes = match encode(&input) {
        Ok(b) => b,
        Err(_) => return,
    };

    // Prepare output buffer (log returns unit, so small buffer is fine)
    let mut output_buf = [0u8; 64];

    // Call the host function
    let _result = unsafe {
        host_log(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            output_buf.as_mut_ptr() as i32,
            output_buf.len() as i32,
        )
    };
}

/// The init function for theater:simple/actor interface.
///
/// Input format: Tuple(Option<List<u8>>, List<u8>)
///   - First element: current state (Option of byte list)
///   - Second element: params (byte list, currently unused)
///
/// Output format: Variant (Result)
///   - tag 0 (Ok): Tuple(Option<List<u8>>, any_result)
///   - tag 1 (Err): String error message
#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    log("Composite test actor: init called!");

    // Parse input: Tuple(state, params)
    let (state, _params) = match input {
        Value::Tuple(items) if items.len() >= 2 => {
            let state = items.into_iter().next().unwrap();
            let params = Value::Tuple(Vec::new()); // Ignore params for now
            (state, params)
        }
        _ => {
            log("Composite test actor: unexpected input format");
            // Return error
            return Value::Variant {
                tag: 1,
                payload: Some(Box::new(Value::String(String::from("Invalid input format")))),
            };
        }
    };

    log("Composite test actor: processing state...");

    // Just pass through the state unchanged for this simple test
    let new_state = state;

    log("Composite test actor: init completed successfully!");

    // Return Ok variant: Variant { tag: 0, payload: Tuple(new_state, result) }
    // The result is just unit for init
    Value::Variant {
        tag: 0,
        payload: Some(Box::new(Value::Tuple(alloc::vec![
            new_state,
            Value::Tuple(Vec::new()), // Unit result
        ]))),
    }
}
