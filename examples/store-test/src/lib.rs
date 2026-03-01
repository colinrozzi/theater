//! Store Test Actor
//!
//! Tests the content-addressable storage handler by:
//! 1. Creating a new store
//! 2. Storing content and verifying the hash
//! 3. Retrieving content and verifying it matches
//! 4. Using labels to reference content
//! 5. Listing labels and calculating size

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

pack_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types!(file = "actor.types");

// ============================================================================
// Host imports
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/runtime", name = "shutdown")]
fn shutdown(data: Option<Vec<u8>>) -> Result<(), String>;

#[import(module = "theater:simple/store", name = "new")]
fn store_new() -> Result<String, String>;

#[import(module = "theater:simple/store", name = "store")]
fn store_content(store_id: String, content: Vec<u8>) -> Result<String, String>;

#[import(module = "theater:simple/store", name = "get")]
fn store_get(store_id: String, content_ref: String) -> Result<Vec<u8>, String>;

#[import(module = "theater:simple/store", name = "exists")]
fn store_exists(store_id: String, content_ref: String) -> Result<bool, String>;

#[import(module = "theater:simple/store", name = "label")]
fn store_label(store_id: String, label: String, content_ref: String) -> Result<(), String>;

#[import(module = "theater:simple/store", name = "get-by-label")]
fn store_get_by_label(store_id: String, label: String) -> Result<Option<String>, String>;

#[import(module = "theater:simple/store", name = "store-at-label")]
fn store_at_label(store_id: String, label: String, content: Vec<u8>) -> Result<String, String>;

#[import(module = "theater:simple/store", name = "list-labels")]
fn store_list_labels(store_id: String) -> Result<Vec<String>, String>;

#[import(module = "theater:simple/store", name = "calculate-total-size")]
fn store_calculate_size(store_id: String) -> Result<u64, String>;

// ============================================================================
// Test Implementation
// ============================================================================

fn run_tests() -> Result<String, String> {
    log(String::from("=== Store Handler Test ==="));

    // Test 1: Create a new store
    log(String::from("Test 1: Creating new store..."));
    let store_id = store_new()?;
    log(format!("  Created store: {}", store_id));

    // Test 2: Store some content
    log(String::from("Test 2: Storing content..."));
    let content1 = b"Hello, Theater Store!".to_vec();
    let ref1 = store_content(store_id.clone(), content1.clone())?;
    log(format!("  Stored content, ref: {}", ref1));

    // Test 3: Check existence
    log(String::from("Test 3: Checking existence..."));
    let exists = store_exists(store_id.clone(), ref1.clone())?;
    if !exists {
        return Err(String::from("Content should exist but doesn't!"));
    }
    log(String::from("  Content exists: true"));

    // Test 4: Retrieve and verify content
    log(String::from("Test 4: Retrieving content..."));
    let retrieved = store_get(store_id.clone(), ref1.clone())?;
    if retrieved != content1 {
        return Err(String::from("Retrieved content doesn't match original!"));
    }
    log(String::from("  Content verified: matches original"));

    // Test 5: Add a label
    log(String::from("Test 5: Adding label..."));
    store_label(store_id.clone(), String::from("greeting"), ref1.clone())?;
    log(String::from("  Added label 'greeting'"));

    // Test 6: Get by label
    log(String::from("Test 6: Getting by label..."));
    let ref_by_label = store_get_by_label(store_id.clone(), String::from("greeting"))?;
    match ref_by_label {
        Some(r) if r == ref1 => log(String::from("  Label resolves correctly")),
        Some(_) => return Err(String::from("Label resolved to wrong ref!")),
        None => return Err(String::from("Label not found!")),
    }

    // Test 7: Store at label (convenience function)
    log(String::from("Test 7: Store at label..."));
    let content2 = b"Second piece of content".to_vec();
    let ref2 = store_at_label(store_id.clone(), String::from("second"), content2)?;
    log(format!("  Stored at label 'second', ref: {}", ref2));

    // Test 8: List labels
    log(String::from("Test 8: Listing labels..."));
    let labels = store_list_labels(store_id.clone())?;
    log(format!("  Labels: {:?}", labels));
    if labels.len() != 2 {
        return Err(format!("Expected 2 labels, got {}", labels.len()));
    }

    // Test 9: Calculate total size
    log(String::from("Test 9: Calculating total size..."));
    let size = store_calculate_size(store_id.clone())?;
    log(format!("  Total size: {} bytes", size));
    // "Hello, Theater Store!" = 21 bytes + "Second piece of content" = 23 bytes = 44 bytes
    if size != 44 {
        return Err(format!("Expected 44 bytes, got {}", size));
    }

    log(String::from("=== All tests passed! ==="));
    Ok(store_id)
}

// ============================================================================
// Exports
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("Store test actor initializing..."));

    match run_tests() {
        Ok(store_id) => {
            log(format!("Tests completed successfully with store: {}", store_id));
            // Shutdown after tests complete
            let _ = shutdown(Some(b"store-test-passed".to_vec()));
            ok_state(state)
        }
        Err(e) => {
            log(format!("TEST FAILED: {}", e));
            let _ = shutdown(Some(format!("store-test-failed: {}", e).into_bytes()));
            err_result(&e)
        }
    }
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
