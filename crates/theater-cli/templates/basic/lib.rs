//! {{project_name}} - A Theater actor using Pack runtime
//!
//! This actor demonstrates the basic structure of a Theater actor using
//! Pack's import/export macros for type-safe WASM interfaces.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use pack_guest::{export, import};

// Set up allocator and panic handler for no_std WASM
pack_guest::setup_guest!();

// Import host functions from theater:simple/runtime
#[import(wit = "theater:simple/runtime.log")]
fn log(msg: String);

#[import(wit = "theater:simple/runtime.shutdown")]
fn shutdown(data: Option<Vec<u8>>) -> Result<(), String>;

/// Initialize the actor.
///
/// This function is called when the actor starts. It receives any existing
/// state and returns the new state to be persisted.
///
/// WIT signature:
///   init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>;
#[export(wit = "theater:simple/actor.init")]
fn init(state: Option<Vec<u8>>) -> Result<(Option<Vec<u8>>,), String> {
    log(String::from("Initializing {{project_name}} actor"));
    log(String::from("Hello from {{project_name}}!"));

    // For this demo, we pass through any existing state unchanged
    // In a real actor, you would deserialize, process, and re-serialize state
    let new_state = state;

    // Shutdown after init - remove this for persistent actors that handle messages
    let _ = shutdown(None);

    Ok((new_state,))
}
