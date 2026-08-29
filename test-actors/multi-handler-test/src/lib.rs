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
use packr_guest::{export, import, pack_types, Value, ValueType};

// Set up allocator and panic handler
packr_guest::setup_guest!();

// Embed interface metadata for hash verification. The supervisor types below
// mirror theater:simple/supervisor exactly (field/case names + order) so the
// host's interface subset hash matches — get this wrong and the actor compiles
// but fails to instantiate.
pack_types! {
    record actor-info {
        id: string,
        name: string,
        parent-id: option<string>,
    }

    variant spawn-failure {
        bad-manifest(string),
        wasm-fetch(string),
        handler-registry(string),
        wasm-invalid(string),
        interface-mismatch(string),
        missing-interface(string),
        missing-metadata(string),
        init-failed(string),
        child-failed(string),
        child-stopped(string),
        timeout(string),
        internal(string),
    }

    variant supervisor-error {
        actor-not-found(string),
        out-of-view(string),
        permission-denied(string),
        invalid-argument(string),
        spawn-failed(spawn-failure),
        runtime-unavailable,
        internal(string),
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/store {
            new: func() -> result<string, string>,
            store: func(store-id: string, content: list<u8>) -> result<string, string>,
            get: func(store-id: string, content-ref: string) -> result<list<u8>, string>,
            store-at-label: func(store-id: string, label: string, content: list<u8>) -> result<string, string>,
            get-by-label: func(store-id: string, label: string) -> result<option<string>, string>,
        }
        theater:simple/supervisor {
            list-actors: func() -> result<list<actor-info>, supervisor-error>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    }
}

// ============================================================================
// Runtime handler imports
// ============================================================================

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

// ============================================================================
// Store handler imports
// ============================================================================

#[import(module = "theater:simple/store", name = "new")]
fn store_new() -> Result<String, String>;

#[import(module = "theater:simple/store", name = "store-at-label")]
fn store_at_label(store_id: String, label: String, data: Vec<u8>) -> Result<String, String>;

// get returns result<list<u8>, string> - handle raw Value.
#[import(module = "theater:simple/store", name = "get")]
fn store_get_raw(store_id: String, content_ref: String) -> Value;

/// Retrieve content bytes by content-ref (the hash returned from store/store-at-label).
fn store_get(store_id: String, content_ref: String) -> Result<Vec<u8>, String> {
    match store_get_raw(store_id, content_ref) {
        // Ok(list<u8>)
        Value::Variant { tag: 0, payload, .. } => match payload.into_iter().next() {
            Some(Value::List { items, .. }) => Ok(items
                .into_iter()
                .filter_map(|v| if let Value::U8(b) = v { Some(b) } else { None })
                .collect()),
            _ => Err(String::from("unexpected ok payload shape")),
        },
        // Err(string)
        Value::Variant { tag: 1, payload, .. } => match payload.into_iter().next() {
            Some(Value::String(e)) => Err(e),
            _ => Err(String::from("unknown error")),
        },
        _ => Err(String::from("unexpected result format")),
    }
}

// ============================================================================
// Supervisor handler imports
// ============================================================================

// Complex return type (result<list<actor-info>, supervisor-error>) - handle raw Value.
#[import(module = "theater:simple/supervisor", name = "list-actors")]
fn list_actors_raw() -> Value;

/// Count the actors in view from a `result<list<actor-info>, supervisor-error>`.
fn list_actors_count() -> Result<usize, String> {
    match list_actors_raw() {
        // Ok(list<actor-info>)
        Value::Variant { tag: 0, payload, .. } => match payload.into_iter().next() {
            Some(Value::List { items, .. }) => Ok(items.len()),
            _ => Err(String::from("unexpected ok payload shape")),
        },
        // Err(supervisor-error)
        Value::Variant { tag: 1, payload, .. } => {
            let case = match payload.into_iter().next() {
                Some(Value::Variant { case_name, .. }) => case_name,
                _ => String::from("unknown"),
            };
            Err(alloc::format!("supervisor-error: {}", case))
        }
        _ => Err(String::from("unexpected result format")),
    }
}

// ============================================================================
// Actor export: init
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    log(String::from("=== Multi-handler test actor starting ==="));

    // The runtime calls init with an empty params tuple; there is no prior
    // state on first init, so default to an empty (none) state.
    let state = match input {
        Value::Tuple(mut items) if !items.is_empty() => items.remove(0),
        _ => empty_state(),
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
    match list_actors_count() {
        Ok(n) => log(alloc::format!(
            "TEST 3: Supervisor handler - PASSED (found {} actors in view)",
            n
        )),
        Err(e) => log(alloc::format!("TEST 3: Supervisor handler - FAILED: {}", e)),
    }

    log(String::from("=== Multi-handler test actor completed ==="));

    // Return success with unchanged state
    ok_state(state)
}

fn test_store_handler() -> Result<(), String> {
    // Create a store
    log(String::from("  Creating content store..."));
    let store_id = store_new()?;
    log(alloc::format!("  Store created: {}", store_id));

    // Store some content at a label; store-at-label returns the content hash.
    let test_data = b"hello world".to_vec();
    log(String::from("  Storing content at label 'test'..."));
    let hash = store_at_label(store_id.clone(), String::from("test"), test_data.clone())?;
    log(alloc::format!("  Content stored with hash: {}", hash));

    // Retrieve it back by content-ref.
    log(String::from("  Retrieving content by content-ref..."));
    let retrieved = store_get(store_id, hash)?;
    if retrieved == test_data {
        log(String::from("  Content retrieved and matches!"));
    } else {
        return Err(String::from("Retrieved content doesn't match"));
    }

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// An empty actor state (`none`), used when init receives no prior state.
fn empty_state() -> Value {
    Value::Option {
        inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
        value: None,
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
