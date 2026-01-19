//! A simple test actor for Composite runtime integration with Theater.
//!
//! This actor:
//! 1. Exports `init` function for the theater:simple/actor interface
//! 2. Imports `log` function from theater:simple/runtime interface
//! 3. Demonstrates basic state handling with WIT+-generated types

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use composite_guest::{export, import, wit, Value};

// Set up allocator and panic handler
composite_guest::setup_guest!();

// Generate types from WIT+ files in the wit/ directory
// This generates: Message, Sexpr, ActorState, InitResult
wit!();

// Import the log function from the host
#[import(module = "theater:simple/runtime")]
fn log(msg: String);

/// Test that the generated types work correctly
fn test_generated_types() {
    // Test Message record
    let msg = Message {
        content: String::from("Hello, World!"),
        timestamp: 12345,
    };
    let _msg_value: Value = msg.clone().into();

    // Test Sexpr variant with recursive self type
    let atom = Sexpr::Atom(String::from("hello"));
    let _atom_value: Value = atom.clone().into();

    // Test nested Sexpr (list of self)
    let nested = Sexpr::List(alloc::vec![
        Box::new(Sexpr::Atom(String::from("a"))),
        Box::new(Sexpr::Atom(String::from("b"))),
    ]);
    let _nested_value: Value = nested.into();

    // Test ActorState
    let empty_state = ActorState::Empty;
    let _: Value = empty_state.into();

    let initialized_state = ActorState::Initialized(msg);
    let _: Value = initialized_state.into();

    // Test InitResult
    let ok_result = InitResult::Ok(ActorState::Empty);
    let _: Value = ok_result.into();

    let err_result = InitResult::Err(String::from("Error!"));
    let _: Value = err_result.into();
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
    log(String::from("Composite test actor: init called!"));

    // Test the generated types
    test_generated_types();
    log(String::from("Generated types test passed!"));

    // Parse input: Tuple(state, params)
    let (state, _params) = match input {
        Value::Tuple(items) if items.len() >= 2 => {
            let state = items.into_iter().next().unwrap();
            let params = Value::Tuple(Vec::new()); // Ignore params for now
            (state, params)
        }
        _ => {
            log(String::from("Composite test actor: unexpected input format"));
            // Return error
            return Value::Variant {
                tag: 1,
                payload: Some(Box::new(Value::String(String::from("Invalid input format")))),
            };
        }
    };

    log(String::from("Composite test actor: processing state..."));

    // Just pass through the state unchanged for this simple test
    let new_state = state;

    log(String::from("Composite test actor: init completed successfully!"));

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
