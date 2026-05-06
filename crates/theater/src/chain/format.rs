//! Human-readable chain format.
//!
//! Events are separated by `---` and use `key: value` lines.
//! Values use pack's Display format for readability.
//!
//! ```text
//! ---
//! hash: cebeeae
//! parent: root
//! type: wasm-call
//! function: theater:simple/actor.init
//! ---
//! hash: 715fa92
//! parent: cebeeae
//! type: host-function
//! interface: theater:simple/runtime
//! function: log
//! input: "[hello] init"
//! output: ()
//! ---
//! hash: 773b86a
//! parent: 715fa92
//! type: wasm-result
//! function: theater:simple/actor.init
//! state: ActorState{greeting: "Hello", count: 0}
//! ```

use std::fmt::Write as _;
use std::io::{self, BufRead};

use crate::chain::ChainEvent;
use crate::pack_bridge;
use packr_abi::{parse_value, Value};

/// Format a chain event as human-readable text.
pub fn format_event(event: &ChainEvent) -> String {
    let mut out = String::new();
    writeln!(out, "---").unwrap();

    // Hash (short)
    let hash = hex::encode(&event.hash);
    writeln!(out, "hash: {}", &hash[..7.min(hash.len())]).unwrap();

    // Parent
    match &event.parent_hash {
        Some(parent) => {
            let ph = hex::encode(parent);
            writeln!(out, "parent: {}", &ph[..7.min(ph.len())]).unwrap();
        }
        None => {
            writeln!(out, "parent: root").unwrap();
        }
    }

    // Decode CGRF data and flatten into readable fields
    if let Ok(value) = pack_bridge::decode_value(&event.data) {
        format_decoded_event(&mut out, &event.event_type, &value);
    } else {
        writeln!(out, "type: {}", event.event_type).unwrap();
        writeln!(out, "data: {} bytes (binary)", event.data.len()).unwrap();
    }

    out
}

/// Format a decoded Value into flattened key: value lines.
fn format_decoded_event(out: &mut String, event_type: &str, value: &Value) {
    // The value is typically:
    // chain-event-payload::wasm(wasm-event-data::wasm-call(wasm-call{...}))
    // chain-event-payload::host-function(host-function-call{...})
    //
    // We want to flatten this into readable fields.

    match value {
        Value::Variant {
            case_name, payload, ..
        } => match case_name.as_str() {
            "wasm" => {
                if let Some(inner) = payload.first() {
                    format_wasm_event(out, inner);
                }
            }
            "host-function" => {
                if let Some(inner) = payload.first() {
                    format_host_function_event(out, inner);
                }
            }
            _ => {
                writeln!(out, "type: {}", event_type).unwrap();
                writeln!(out, "data: {}", value).unwrap();
            }
        },
        _ => {
            writeln!(out, "type: {}", event_type).unwrap();
            writeln!(out, "data: {}", value).unwrap();
        }
    }
}

fn format_wasm_event(out: &mut String, value: &Value) {
    match value {
        Value::Variant {
            case_name, payload, ..
        } => {
            match case_name.as_str() {
                "wasm-call" => {
                    writeln!(out, "type: call").unwrap();
                    if let Some(Value::Record { fields, .. }) = payload.first() {
                        for (name, val) in fields {
                            match name.as_str() {
                                "function-name" => {
                                    writeln!(out, "function: {}", val).unwrap();
                                }
                                "params" => {
                                    // Params are raw bytes, try to decode
                                    if let Value::List { items, .. } = val {
                                        let bytes: Vec<u8> = items
                                            .iter()
                                            .filter_map(|v| {
                                                if let Value::U8(b) = v {
                                                    Some(*b)
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect();
                                        if let Ok(decoded) = pack_bridge::decode_value(&bytes) {
                                            writeln!(out, "params: {}", decoded).unwrap();
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "wasm-result" => {
                    writeln!(out, "type: result").unwrap();
                    if let Some(Value::Record { fields, .. }) = payload.first() {
                        for (name, val) in fields {
                            match name.as_str() {
                                "function-name" => {
                                    writeln!(out, "function: {}", val).unwrap();
                                }
                                "state" => {
                                    writeln!(out, "state: {}", val).unwrap();
                                }
                                "bytes" => {} // skip raw bytes
                                _ => {}
                            }
                        }
                    }
                }
                "wasm-error" => {
                    writeln!(out, "type: error").unwrap();
                    if let Some(Value::Record { fields, .. }) = payload.first() {
                        for (name, val) in fields {
                            match name.as_str() {
                                "function-name" => {
                                    writeln!(out, "function: {}", val).unwrap();
                                }
                                "message" => {
                                    writeln!(out, "message: {}", val).unwrap();
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {
                    writeln!(out, "type: wasm/{}", case_name).unwrap();
                    writeln!(out, "data: {}", value).unwrap();
                }
            }
        }
        _ => {
            writeln!(out, "type: wasm").unwrap();
            writeln!(out, "data: {}", value).unwrap();
        }
    }
}

fn format_host_function_event(out: &mut String, value: &Value) {
    writeln!(out, "type: host-call").unwrap();
    if let Value::Record { fields, .. } = value {
        for (name, val) in fields {
            match name.as_str() {
                "interface" => writeln!(out, "interface: {}", val).unwrap(),
                "function" => writeln!(out, "function: {}", val).unwrap(),
                "input" => writeln!(out, "input: {}", val).unwrap(),
                "output" => {
                    // Skip empty tuple output
                    if !matches!(val, Value::Tuple(items) if items.is_empty()) {
                        writeln!(out, "output: {}", val).unwrap();
                    }
                }
                _ => writeln!(out, "{}: {}", name, val).unwrap(),
            }
        }
    }
}

/// Format an entire chain file (list of events).
pub fn format_chain(events: &[ChainEvent]) -> String {
    events.iter().map(format_event).collect::<String>()
}

/// Parse events from the human-readable chain format.
///
/// Values in fields (input, output, state, params, data) use pack's value literal
/// syntax and can be parsed back into `Value` via `ParsedEvent::parse_field()`.
pub fn parse_events(reader: &mut dyn BufRead) -> io::Result<Vec<ParsedEvent>> {
    let mut events = Vec::new();
    let mut current: Option<ParsedEvent> = None;

    for line in reader.lines() {
        let line = line?;
        if line == "---" {
            if let Some(event) = current.take() {
                events.push(event);
            }
            current = Some(ParsedEvent::default());
        } else if let Some(ref mut event) = current {
            if let Some((key, value)) = line.split_once(": ") {
                match key {
                    "hash" => event.hash = value.to_string(),
                    "parent" => event.parent = value.to_string(),
                    "type" => event.event_type = value.to_string(),
                    "function" => event.function = Some(value.to_string()),
                    "interface" => event.interface = Some(value.to_string()),
                    "input" => event.input = Some(value.to_string()),
                    "output" => event.output = Some(value.to_string()),
                    "state" => event.state = Some(value.to_string()),
                    "message" => event.message = Some(value.to_string()),
                    "params" => event.params = Some(value.to_string()),
                    "data" => event.data = Some(value.to_string()),
                    _ => {}
                }
            }
        }
    }
    if let Some(event) = current.take() {
        events.push(event);
    }

    Ok(events)
}

/// A parsed event from the human-readable format.
///
/// String fields (input, output, state, params) contain value literals
/// that can be parsed into `Value` via the `parse_*` methods.
#[derive(Debug, Default, Clone)]
pub struct ParsedEvent {
    pub hash: String,
    pub parent: String,
    pub event_type: String,
    pub function: Option<String>,
    pub interface: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub state: Option<String>,
    pub message: Option<String>,
    pub params: Option<String>,
    pub data: Option<String>,
}

impl ParsedEvent {
    /// Parse the input field as a Value.
    pub fn parse_input(&self) -> Option<Result<Value, packr_abi::ParseError>> {
        self.input.as_deref().map(parse_value)
    }

    /// Parse the output field as a Value.
    pub fn parse_output(&self) -> Option<Result<Value, packr_abi::ParseError>> {
        self.output.as_deref().map(parse_value)
    }

    /// Parse the state field as a Value.
    pub fn parse_state(&self) -> Option<Result<Value, packr_abi::ParseError>> {
        self.state.as_deref().map(parse_value)
    }

    /// Parse the params field as a Value.
    pub fn parse_params(&self) -> Option<Result<Value, packr_abi::ParseError>> {
        self.params.as_deref().map(parse_value)
    }
}
