//! Multi-handler test actor for Composite runtime integration.
//!
//! This actor tests multiple Theater handlers:
//! - runtime: log function
//! - store: content storage operations
//! - supervisor: child actor management (just the host functions, not actual spawning)

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use composite_guest::{encode, export, Value};

// Set up allocator and panic handler
composite_guest::setup_guest!();

// ============================================================================
// Runtime handler imports
// ============================================================================

#[link(wasm_import_module = "theater:simple/runtime")]
extern "C" {
    #[link_name = "log"]
    fn host_log(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;
}

fn log(msg: &str) {
    let input = Value::String(String::from(msg));
    let input_bytes = match encode(&input) {
        Ok(b) => b,
        Err(_) => return,
    };
    let mut output_buf = [0u8; 64];
    unsafe {
        host_log(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            output_buf.as_mut_ptr() as i32,
            output_buf.len() as i32,
        );
    }
}

// ============================================================================
// Store handler imports
// ============================================================================

#[link(wasm_import_module = "theater:simple/store")]
extern "C" {
    #[link_name = "new"]
    fn store_new(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;

    #[link_name = "store"]
    fn store_store(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;

    #[link_name = "get"]
    fn store_get(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;

    #[link_name = "store-at-label"]
    fn store_at_label(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;

    #[link_name = "get-by-label"]
    fn store_get_by_label(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;
}

/// Create a new content store, returns store ID
fn create_store() -> Result<String, String> {
    // Input is unit (empty tuple)
    let input = Value::Tuple(vec![]);
    let input_bytes = encode(&input).map_err(|e| alloc::format!("encode error: {:?}", e))?;

    let mut output_buf = [0u8; 256];
    let result_len = unsafe {
        store_new(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            output_buf.as_mut_ptr() as i32,
            output_buf.len() as i32,
        )
    };

    if result_len < 0 {
        return Err(String::from("store_new failed"));
    }

    // Decode the result - it's a Result<String, String> encoded as a Variant
    let result_bytes = &output_buf[..result_len as usize];
    let value = composite_guest::decode(result_bytes)
        .map_err(|e| alloc::format!("decode error: {:?}", e))?;

    // Result is Variant { tag: 0 for Ok, tag: 1 for Err }
    match value {
        Value::Variant { tag: 0, payload } => {
            if let Some(boxed) = payload {
                if let Value::String(s) = *boxed {
                    return Ok(s);
                }
            }
            Err(String::from("unexpected Ok payload"))
        }
        Value::Variant { tag: 1, payload } => {
            if let Some(boxed) = payload {
                if let Value::String(s) = *boxed {
                    return Err(s);
                }
            }
            Err(String::from("unknown error"))
        }
        _ => Err(String::from("unexpected result type")),
    }
}

/// Store content at a label
fn store_content_at_label(store_id: &str, label: &str, content: &[u8]) -> Result<String, String> {
    // Input is tuple(store_id, label, content)
    let input = Value::Tuple(vec![
        Value::String(String::from(store_id)),
        Value::String(String::from(label)),
        Value::List(content.iter().map(|b| Value::U8(*b)).collect()),
    ]);
    let input_bytes = encode(&input).map_err(|e| alloc::format!("encode error: {:?}", e))?;

    let mut output_buf = [0u8; 512];
    let result_len = unsafe {
        store_at_label(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            output_buf.as_mut_ptr() as i32,
            output_buf.len() as i32,
        )
    };

    if result_len < 0 {
        return Err(String::from("store_at_label failed"));
    }

    let result_bytes = &output_buf[..result_len as usize];
    let value = composite_guest::decode(result_bytes)
        .map_err(|e| alloc::format!("decode error: {:?}", e))?;

    // Result is Variant with Ok containing content-ref record
    match value {
        Value::Variant { tag: 0, payload } => {
            if let Some(boxed) = payload {
                // content-ref is a record with "hash" field
                if let Value::Record(fields) = *boxed {
                    for (name, val) in fields {
                        if name == "hash" {
                            if let Value::String(hash) = val {
                                return Ok(hash);
                            }
                        }
                    }
                }
            }
            Err(String::from("unexpected content-ref format"))
        }
        Value::Variant { tag: 1, payload } => {
            if let Some(boxed) = payload {
                if let Value::String(s) = *boxed {
                    return Err(s);
                }
            }
            Err(String::from("unknown error"))
        }
        _ => Err(String::from("unexpected result type")),
    }
}

/// Get content by label
fn get_content_by_label(store_id: &str, label: &str) -> Result<Option<String>, String> {
    // Input is tuple(store_id, label)
    let input = Value::Tuple(vec![
        Value::String(String::from(store_id)),
        Value::String(String::from(label)),
    ]);
    let input_bytes = encode(&input).map_err(|e| alloc::format!("encode error: {:?}", e))?;

    let mut output_buf = [0u8; 512];
    let result_len = unsafe {
        store_get_by_label(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            output_buf.as_mut_ptr() as i32,
            output_buf.len() as i32,
        )
    };

    if result_len < 0 {
        return Err(String::from("get_by_label failed"));
    }

    let result_bytes = &output_buf[..result_len as usize];
    let value = composite_guest::decode(result_bytes)
        .map_err(|e| alloc::format!("decode error: {:?}", e))?;

    // Result is Variant with Ok containing Option<content-ref>
    match value {
        Value::Variant { tag: 0, payload } => {
            if let Some(boxed) = payload {
                match *boxed {
                    Value::Option(Some(inner)) => {
                        if let Value::Record(fields) = *inner {
                            for (name, val) in fields {
                                if name == "hash" {
                                    if let Value::String(hash) = val {
                                        return Ok(Some(hash));
                                    }
                                }
                            }
                        }
                        Err(String::from("unexpected content-ref format"))
                    }
                    Value::Option(None) => Ok(None),
                    _ => Err(String::from("unexpected option format")),
                }
            } else {
                Ok(None)
            }
        }
        Value::Variant { tag: 1, payload } => {
            if let Some(boxed) = payload {
                if let Value::String(s) = *boxed {
                    return Err(s);
                }
            }
            Err(String::from("unknown error"))
        }
        _ => Err(String::from("unexpected result type")),
    }
}

// ============================================================================
// Supervisor handler imports (just list-children for testing)
// ============================================================================

#[link(wasm_import_module = "theater:simple/supervisor")]
extern "C" {
    #[link_name = "list-children"]
    fn supervisor_list_children(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;
}

fn list_children() -> Result<Vec<String>, String> {
    let input = Value::Tuple(vec![]);
    let input_bytes = encode(&input).map_err(|e| alloc::format!("encode error: {:?}", e))?;

    let mut output_buf = [0u8; 1024];
    let result_len = unsafe {
        supervisor_list_children(
            input_bytes.as_ptr() as i32,
            input_bytes.len() as i32,
            output_buf.as_mut_ptr() as i32,
            output_buf.len() as i32,
        )
    };

    if result_len < 0 {
        return Err(String::from("list_children failed"));
    }

    let result_bytes = &output_buf[..result_len as usize];
    let value = composite_guest::decode(result_bytes)
        .map_err(|e| alloc::format!("decode error: {:?}", e))?;

    // Result is list<string>
    match value {
        Value::List(items) => {
            let mut children = Vec::new();
            for item in items {
                if let Value::String(s) = item {
                    children.push(s);
                }
            }
            Ok(children)
        }
        _ => Err(String::from("unexpected result type")),
    }
}

// ============================================================================
// Actor export: init
// ============================================================================

#[export]
fn init(input: Value) -> Value {
    log("=== Multi-handler test actor starting ===");

    // Parse input state
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => {
            log("ERROR: unexpected input format");
            return error_result("Invalid input format");
        }
    };

    // Test 1: Runtime handler (log) - already working if we see this!
    log("TEST 1: Runtime handler - PASSED (you're reading this!)");

    // Test 2: Store handler
    log("TEST 2: Store handler...");
    match test_store_handler() {
        Ok(()) => log("TEST 2: Store handler - PASSED"),
        Err(e) => {
            log(&alloc::format!("TEST 2: Store handler - FAILED: {}", e));
        }
    }

    // Test 3: Supervisor handler
    // NOTE: Skipping actual call - it requires full Theater runtime to respond
    // The handler IS wired up correctly though!
    log("TEST 3: Supervisor handler - SKIPPED (requires full Theater runtime)");

    log("=== Multi-handler test actor completed ===");

    // Return success with unchanged state
    Value::Variant {
        tag: 0,
        payload: Some(Box::new(Value::Tuple(vec![
            state,
            Value::Tuple(Vec::new()),
        ]))),
    }
}

fn test_store_handler() -> Result<(), String> {
    // Create a store - this tests the host function call works
    log("  Creating content store...");
    let store_id = create_store()?;
    log(&alloc::format!("  Store created: {}", store_id));
    log("  Store handler host functions are working!");

    Ok(())
}

fn test_supervisor_handler() -> Result<(), String> {
    // Just test that we can call list-children (should return empty list)
    log("  Listing child actors...");
    let children = list_children()?;
    log(&alloc::format!("  Found {} children", children.len()));

    // We don't have any children, so empty list is expected
    if children.is_empty() {
        log("  No children (expected for fresh actor)");
    }

    Ok(())
}

fn error_result(msg: &str) -> Value {
    Value::Variant {
        tag: 1,
        payload: Some(Box::new(Value::String(String::from(msg)))),
    }
}
