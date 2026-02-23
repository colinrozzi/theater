//! Multi-handler test actor for Pack runtime integration.
//!
//! This actor tests multiple Theater handlers:
//! - runtime: log function
//! - store: content storage operations
//! - supervisor: child actor management

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

// Set up allocator and panic handler
pack_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/store {
            new: func() -> result<string, string>,
            store: func(store-id: string, data: list<u8>) -> result<string, string>,
            get: func(store-id: string, content-hash: string) -> result<option<list<u8>>, string>,
            store-at-label: func(store-id: string, label: string, data: list<u8>) -> result<string, string>,
            get-by-label: func(store-id: string, label: string) -> result<option<list<u8>>, string>,
        }
        theater:simple/supervisor {
            list-children: func() -> list<string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    }
}

// ============================================================================
// Runtime handler imports
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

// ============================================================================
// Store handler imports
// ============================================================================

#[import(module = "theater:simple/store", name = "new")]
fn store_new() -> Result<String, String>;

#[import(module = "theater:simple/store", name = "store-at-label")]
fn store_at_label(store_id: String, label: String, data: Vec<u8>) -> Result<String, String>;

// Complex return type - handle raw Value
#[import(module = "theater:simple/store", name = "get-by-label")]
fn store_get_by_label_raw(store_id: String, label: String) -> Value;

fn store_get_by_label(store_id: String, label: String) -> Result<Option<Vec<u8>>, String> {
    let result = store_get_by_label_raw(store_id, label);
    // Parse Result<Option<list<u8>>, string> from Value
    match result {
        Value::Variant { tag: 0, payload, .. } => {
            // Ok case
            if let Some(inner) = payload.into_iter().next() {
                match inner {
                    Value::Option { value: Some(data_val), .. } => {
                        if let Value::List { items, .. } = *data_val {
                            let bytes: Vec<u8> = items.into_iter().filter_map(|v| {
                                if let Value::U8(b) = v { Some(b) } else { None }
                            }).collect();
                            Ok(Some(bytes))
                        } else {
                            Err(String::from("unexpected data format"))
                        }
                    }
                    Value::Option { value: None, .. } => Ok(None),
                    _ => Err(String::from("unexpected option format")),
                }
            } else {
                Ok(None)
            }
        }
        Value::Variant { tag: 1, payload, .. } => {
            // Err case
            if let Some(Value::String(e)) = payload.into_iter().next() {
                Err(e)
            } else {
                Err(String::from("unknown error"))
            }
        }
        _ => Err(String::from("unexpected result format")),
    }
}

// ============================================================================
// Supervisor handler imports
// ============================================================================

#[import(module = "theater:simple/supervisor", name = "list-children")]
fn list_children() -> Vec<String>;

// ============================================================================
// Actor export: init
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    log(String::from("=== Multi-handler test actor starting ==="));

    // Parse input state
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            log(String::from("ERROR: unexpected input format"));
            return err_result("Invalid input format");
        }
    };

    // Test 1: Runtime handler (log) - already working if we see this!
    log(String::from("TEST 1: Runtime handler - PASSED (you're reading this!)"));

    // Test 2: Store handler
    log(String::from("TEST 2: Store handler..."));
    match test_store_handler() {
        Ok(()) => log(String::from("TEST 2: Store handler - PASSED")),
        Err(e) => {
            log(alloc::format!("TEST 2: Store handler - FAILED: {}", e));
        }
    }

    // Test 3: Supervisor handler
    log(String::from("TEST 3: Supervisor handler..."));
    let children = list_children();
    log(alloc::format!("TEST 3: Supervisor handler - PASSED (found {} children)", children.len()));

    log(String::from("=== Multi-handler test actor completed ==="));

    // Return success with unchanged state
    ok_state(state)
}

fn test_store_handler() -> Result<(), String> {
    // Create a store
    log(String::from("  Creating content store..."));
    let store_id = store_new()?;
    log(alloc::format!("  Store created: {}", store_id));

    // Store some content at a label
    let test_data = b"hello world".to_vec();
    log(String::from("  Storing content at label 'test'..."));
    let hash = store_at_label(store_id.clone(), String::from("test"), test_data.clone())?;
    log(alloc::format!("  Content stored with hash: {}", hash));

    // Retrieve it back
    log(String::from("  Retrieving content by label..."));
    let retrieved = store_get_by_label(store_id, String::from("test"))?;
    match retrieved {
        Some(data) if data == test_data => {
            log(String::from("  Content retrieved and matches!"));
        }
        Some(_) => {
            return Err(String::from("Retrieved content doesn't match"));
        }
        None => {
            return Err(String::from("Content not found"));
        }
    }

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

fn err_result(msg: &str) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(vec![]),
        err_type: ValueType::String,
        value: Err(alloc::boxed::Box::new(Value::String(String::from(msg)))),
    }
}

fn ok_state(state: Value) -> Value {
    let inner = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(inner)),
    }
}
