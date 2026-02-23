//! TCP echo test actor for Pack runtime.
//!
//! A simple actor that:
//! - Imports `theater:simple/runtime.log` for logging
//! - Imports `theater:simple/tcp.{send, receive, close}` for TCP I/O
//! - Exports `theater:simple/actor.init`
//! - Exports `theater:simple/tcp-client.handle-connection`
//!
//! On each incoming connection it reads data, echoes it back, then closes.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

pack_guest::setup_guest!();

// Embed interface metadata for hash verification (loaded from actor.types)
pack_types!(file = "actor.types");

// ============================================================================
// Host imports — the #[import] macro generates all FFI boilerplate:
//   - extern "C" block with the raw import
//   - Value encoding/decoding
//   - indirect-pointer calling convention
// ============================================================================

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/tcp", name = "send")]
fn tcp_send(connection_id: String, data: Vec<u8>) -> Result<u64, String>;

#[import(module = "theater:simple/tcp", name = "receive")]
fn tcp_receive(connection_id: String, max_bytes: u32) -> Result<Vec<u8>, String>;

#[import(module = "theater:simple/tcp", name = "close")]
fn tcp_close(connection_id: String) -> Result<(), String>;

// ============================================================================
// Exports
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("tcp-echo: init"));
    ok_state(state)
}

/// handle-connection(state: option<list<u8>>, params: tuple<string>)
///   -> result<tuple<option<list<u8>>>, string>
#[export(name = "theater:simple/tcp-client.handle-connection")]
fn handle_connection(input: Value) -> Value {
    // Input is (state, params) where params = tuple<conn_id: string>
    let (state, conn_id) = match input {
        Value::Tuple(mut items) if items.len() >= 2 => {
            let params = items.remove(1);
            let state = items.remove(0);
            let conn_id = match params {
                Value::Tuple(mut p) if !p.is_empty() => match p.remove(0) {
                    Value::String(s) => s,
                    _ => return err_result("Expected string connection id"),
                },
                Value::String(s) => s,
                _ => return err_result("Expected params tuple"),
            };
            (state, conn_id)
        }
        _ => return err_result("Invalid input format"),
    };

    log(String::from("tcp-echo: new connection"));

    // Read up to 4096 bytes
    let data = match tcp_receive(conn_id.clone(), 4096) {
        Ok(d) => d,
        Err(e) => {
            log(String::from("tcp-echo: receive error"));
            let _ = tcp_close(conn_id);
            return err_result_string(&e);
        }
    };

    if data.is_empty() {
        log(String::from("tcp-echo: client disconnected (EOF)"));
        let _ = tcp_close(conn_id);
        return ok_state(state);
    }

    // Echo the data back
    match tcp_send(conn_id.clone(), data) {
        Ok(_) => {
            log(String::from("tcp-echo: echoed bytes"));
        }
        Err(e) => {
            log(String::from("tcp-echo: send error"));
            let _ = tcp_close(conn_id);
            return err_result_string(&e);
        }
    }

    let _ = tcp_close(conn_id);
    log(String::from("tcp-echo: connection closed"));

    ok_state(state)
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

fn err_result_string(msg: &String) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(vec![]),
        err_type: ValueType::String,
        value: Err(alloc::boxed::Box::new(Value::String(msg.clone()))),
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
