//! `store` — content-addressed storage via `theater:simple/store`.
//!
//! On init this actor demonstrates the store lifecycle: open a store, put some
//! bytes (getting back a content hash), read them back, then attach a human
//! label and resolve it. Content is addressed by hash, so identical bytes
//! dedupe to the same ref.
//!
//! Only the store functions this actor uses are declared; the interface
//! subset-hash lets an actor import a subset of the host interface.

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use packr_guest::{export, import, pack_types, Value, ValueType};

packr_guest::setup_guest!();

pack_types! {
    record store-state {
        store-id: string,
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/store {
            new: func() -> result<string, string>,
            store: func(store-id: string, content: list<u8>) -> result<string, string>,
            get: func(store-id: string, content-ref: string) -> result<list<u8>, string>,
            label: func(store-id: string, label: string, content-ref: string) -> result<_, string>,
            get-by-label: func(store-id: string, label: string) -> result<option<string>, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<store-state, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/store", name = "new")]
fn store_new() -> Value;
#[import(module = "theater:simple/store", name = "store")]
fn store_put(store_id: String, content: Value) -> Value;
#[import(module = "theater:simple/store", name = "get")]
fn store_get(store_id: String, content_ref: String) -> Value;
#[import(module = "theater:simple/store", name = "label")]
fn store_label(store_id: String, label: String, content_ref: String) -> Value;
#[import(module = "theater:simple/store", name = "get-by-label")]
fn store_get_by_label(store_id: String, label: String) -> Value;

/// Pull the `ok` payload (tag 0) out of a `result<T, string>` Value.
fn ok_payload(v: Value) -> Option<Value> {
    match v {
        Value::Variant { tag: 0, payload, .. } => payload.into_iter().next(),
        Value::Result { value: Ok(inner), .. } => Some(*inner),
        _ => None,
    }
}

fn as_string(v: Option<Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

fn bytes(content: &str) -> Value {
    Value::List {
        elem_type: ValueType::U8,
        items: content.bytes().map(Value::U8).collect(),
    }
}

fn ok_state(store_id: &str) -> Value {
    let state = Value::Record {
        type_name: String::from("store-state"),
        fields: vec![(
            String::from("store-id"),
            Value::String(String::from(store_id)),
        )],
    };
    Value::Result {
        ok_type: state.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(state)),
    }
}

#[export(name = "theater:simple/actor.init")]
fn init(_input: Value) -> Value {
    let store_id = match as_string(ok_payload(store_new())) {
        Some(id) => id,
        None => {
            log(String::from("store: failed to open a store"));
            return ok_state("");
        }
    };
    log(format!("store: opened store {}", store_id));

    // Put some content; the store returns its content hash.
    let content_ref = match as_string(ok_payload(store_put(store_id.clone(), bytes("hello, theater")))) {
        Some(r) => r,
        None => return ok_state(&store_id),
    };
    log(format!("store: stored content -> {}", content_ref));

    // Read it back.
    if ok_payload(store_get(store_id.clone(), content_ref.clone())).is_some() {
        log(String::from("store: read the content back by ref"));
    }

    // Attach a label and resolve it.
    let _ = store_label(store_id.clone(), String::from("greeting"), content_ref.clone());
    if let Some(Value::Option { value: Some(inner), .. }) =
        ok_payload(store_get_by_label(store_id.clone(), String::from("greeting")))
    {
        if let Value::String(r) = *inner {
            log(format!("store: label 'greeting' resolves to {}", r));
        }
    }

    ok_state(&store_id)
}
