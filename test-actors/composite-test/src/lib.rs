//! A simple test actor for Composite runtime integration with Theater.
//!
//! This actor:
//! 1. Exports `init` function for the theater:simple/actor interface
//! 2. Imports `log` function from theater:simple/runtime interface
//! 3. Demonstrates basic state handling with WIT+-generated types
//! 4. Uses typed function parameters (not raw Value)

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

// Import the log function from the host using WIT path
#[import(wit = "theater:simple/runtime.log")]
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
/// Now using typed parameters thanks to the export macro!
/// The WIT signature is:
///   init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>;
///
/// The macro automatically:
/// - Extracts `state` from the input Value
/// - Wraps the Result return value back to Value
#[export(wit = "theater:simple/actor.init")]
fn init(state: Option<Vec<u8>>) -> Result<(Option<Vec<u8>>,), String> {
    log(String::from("Composite test actor: init called with typed params!"));

    // Test the generated types
    test_generated_types();
    log(String::from("Generated types test passed!"));

    log(String::from("Composite test actor: processing state..."));

    // Just pass through the state unchanged for this simple test
    let new_state = state;

    log(String::from("Composite test actor: init completed successfully!"));

    // Return the new state wrapped in Ok
    // The type is Result<(Option<Vec<u8>>,), String> matching the WIT signature
    Ok((new_state,))
}
