//! Terminal echo test actor for Pack runtime.
//!
//! A simple actor that demonstrates terminal I/O:
//! - Imports `theater:simple/runtime.log` for logging
//! - Imports `theater:simple/terminal.{write-stdout, write-stderr, set-raw-mode, get-size}` for terminal I/O
//! - Exports `theater:simple/actor.init`
//! - Exports `theater:simple/terminal.{handle-input, handle-signal, handle-resize}`
//!
//! Echoes all input back to stdout. Press 'q' or Ctrl-C to exit.

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

#[import(module = "theater:simple/runtime", name = "shutdown")]
fn shutdown(data: Option<Vec<u8>>) -> Result<(), String>;

#[import(module = "theater:simple/terminal", name = "write-stdout")]
fn write_stdout(data: Vec<u8>) -> Result<u64, String>;

#[import(module = "theater:simple/terminal", name = "write-stderr")]
fn write_stderr(data: Vec<u8>) -> Result<u64, String>;

#[import(module = "theater:simple/terminal", name = "set-raw-mode")]
fn set_raw_mode(enabled: bool) -> Result<(), String>;

#[import(module = "theater:simple/terminal", name = "get-size")]
fn get_size() -> Result<(u16, u16), String>;

// ============================================================================
// Exports
// ============================================================================

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("terminal-echo: initializing"));

    // Get terminal size
    let size_str = match get_size() {
        Ok((cols, rows)) => alloc::format!("{}x{}", cols, rows),
        Err(_) => String::from("unknown"),
    };

    // Print welcome message
    let welcome = alloc::format!(
        "\r\n=== Terminal Echo Actor ===\r\n\
         Terminal size: {}\r\n\
         Type anything to echo. Press 'q' or Ctrl-C to exit.\r\n\
         \r\n> ",
        size_str
    );
    let _ = write_stdout(welcome.into_bytes());

    log(String::from("terminal-echo: ready"));

    ok_state(state)
}

/// handle-input(state: option<list<u8>>, data: list<u8>)
///   -> result<tuple<option<list<u8>>>, string>
#[export(name = "theater:simple/terminal.handle-input")]
fn handle_input(input: Value) -> Value {
    // Input is (state, data) where data = list<u8>
    let (state, data) = match input {
        Value::Tuple(mut items) if items.len() >= 2 => {
            let data_val = items.remove(1);
            let state = items.remove(0);
            let data = match data_val {
                Value::List { items, .. } => {
                    items.iter().filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    }).collect::<Vec<u8>>()
                }
                _ => return err_result("Expected list<u8> for data"),
            };
            (state, data)
        }
        _ => return err_result("Invalid input format"),
    };

    // Check for quit signals
    for &byte in &data {
        // 'q' to quit
        if byte == b'q' || byte == b'Q' {
            let _ = write_stdout(b"\r\n\r\nGoodbye!\r\n".to_vec());
            let _ = shutdown(None);
            return ok_state(state);
        }
        // Ctrl-C (ETX)
        if byte == 0x03 {
            let _ = write_stdout(b"\r\n\r\n^C - Interrupted\r\n".to_vec());
            let _ = shutdown(None);
            return ok_state(state);
        }
        // Ctrl-D (EOT)
        if byte == 0x04 {
            let _ = write_stdout(b"\r\n\r\n^D - Exit\r\n".to_vec());
            let _ = shutdown(None);
            return ok_state(state);
        }
    }

    // Echo the input
    // For a nicer display, handle special characters
    let mut output: Vec<u8> = Vec::new();
    for &byte in &data {
        match byte {
            // Enter key - echo newline and prompt
            b'\r' | b'\n' => {
                output.extend_from_slice(b"\r\n> ");
            }
            // Backspace - move cursor back, erase, move back
            0x7f | 0x08 => {
                output.extend_from_slice(b"\x08 \x08");
            }
            // Escape sequences (just pass through for now)
            0x1b => {
                output.push(byte);
            }
            // Printable characters
            0x20..=0x7e => {
                output.push(byte);
            }
            // Non-printable - show as hex
            _ => {
                let hex = alloc::format!("[0x{:02x}]", byte);
                output.extend_from_slice(hex.as_bytes());
            }
        }
    }

    if !output.is_empty() {
        let _ = write_stdout(output);
    }

    ok_state(state)
}

/// handle-signal(state: option<list<u8>>, signal: string)
///   -> result<tuple<option<list<u8>>>, string>
#[export(name = "theater:simple/terminal.handle-signal")]
fn handle_signal(input: Value) -> Value {
    // Input is (state, signal) where signal = string
    let (state, signal) = match input {
        Value::Tuple(mut items) if items.len() >= 2 => {
            let signal_val = items.remove(1);
            let state = items.remove(0);
            let signal = match signal_val {
                Value::String(s) => s,
                _ => return err_result("Expected string for signal"),
            };
            (state, signal)
        }
        _ => return err_result("Invalid input format"),
    };

    log(alloc::format!("terminal-echo: received signal: {}", signal));

    match signal.as_str() {
        "interrupt" => {
            let _ = write_stdout(b"\r\n\r\n^C - Interrupted via signal\r\n".to_vec());
            let _ = shutdown(None);
        }
        "terminate" => {
            let _ = write_stdout(b"\r\n\r\nTerminated via signal\r\n".to_vec());
            let _ = shutdown(None);
        }
        _ => {
            let msg = alloc::format!("\r\n[Signal: {}]\r\n> ", signal);
            let _ = write_stdout(msg.into_bytes());
        }
    }

    ok_state(state)
}

/// handle-resize(state: option<list<u8>>, params: tuple<u16, u16>)
///   -> result<tuple<option<list<u8>>>, string>
#[export(name = "theater:simple/terminal.handle-resize")]
fn handle_resize(input: Value) -> Value {
    // Input is (state, params) where params = (cols, rows)
    let (state, cols, rows) = match input {
        Value::Tuple(mut items) if items.len() >= 2 => {
            let params = items.remove(1);
            let state = items.remove(0);
            let (cols, rows) = match params {
                Value::Tuple(mut p) if p.len() >= 2 => {
                    let rows = match p.remove(1) {
                        Value::U16(r) => r,
                        _ => return err_result("Expected u16 for rows"),
                    };
                    let cols = match p.remove(0) {
                        Value::U16(c) => c,
                        _ => return err_result("Expected u16 for cols"),
                    };
                    (cols, rows)
                }
                _ => return err_result("Expected tuple<u16, u16> for resize params"),
            };
            (state, cols, rows)
        }
        _ => return err_result("Invalid input format"),
    };

    log(alloc::format!("terminal-echo: resize to {}x{}", cols, rows));

    let msg = alloc::format!("\r\n[Resized: {}x{}]\r\n> ", cols, rows);
    let _ = write_stdout(msg.into_bytes());

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

fn ok_state(state: Value) -> Value {
    let inner = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(inner)),
    }
}
