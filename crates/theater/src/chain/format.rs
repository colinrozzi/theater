//! Lossless human-readable chain format.
//!
//! Each event is three lines after the `---` separator:
//! 1. `hash: <full hex>`
//! 2. `parent: <full hex>` or `parent: root`
//! 3. The Value literal of the entire event payload (one line)
//!
//! ```text
//! ---
//! hash: 752f94aee6d7d32bccb1794cd22d62ad0f579709
//! parent: root
//! chain-event-payload::wasm(wasm-event-data::wasm-call(wasm-call{function-name: "theater:simple/actor.init", params: ()}))
//! ---
//! hash: b9aa0dcb36a9e1fb4c4842611cfa272c8f629016
//! parent: 752f94aee6d7d32bccb1794cd22d62ad0f579709
//! chain-event-payload::host-function(host-function-call{interface: "theater:simple/runtime", function: "log", input: "[hello] init", output: ()})
//! ```
//!
//! To reconstruct: parse the Value literal → encode to CGRF → get exact original bytes and hash.

use std::fmt::Write as _;
use std::io::{self, BufRead};

use crate::chain::ChainEvent;
use crate::pack_bridge;
use packr_abi::parse_value;

/// Format a chain event as a lossless Value literal.
pub fn format_event(event: &ChainEvent) -> String {
    let mut out = String::new();
    writeln!(out, "---").unwrap();
    writeln!(out, "hash: {}", hex::encode(&event.hash)).unwrap();

    match &event.parent_hash {
        Some(parent) => writeln!(out, "parent: {}", hex::encode(parent)).unwrap(),
        None => writeln!(out, "parent: root").unwrap(),
    }

    // Decode CGRF data to Value and display as literal
    if let Ok(value) = pack_bridge::decode_value(&event.data) {
        writeln!(out, "{}", value).unwrap();
    } else if !event.data.is_empty() {
        writeln!(out, "raw: {} bytes", event.data.len()).unwrap();
    }

    out
}

/// Format an entire chain file (list of events).
pub fn format_chain(events: &[ChainEvent]) -> String {
    events.iter().map(format_event).collect::<String>()
}

/// Parse a chain file back into ChainEvents.
///
/// Each event block starts with `---`, then hash/parent lines, then
/// a Value literal line. The Value is re-encoded to CGRF to produce
/// the `data` field, enabling full hash verification during replay.
pub fn parse_events(reader: &mut dyn BufRead) -> io::Result<Vec<ChainEvent>> {
    let mut events = Vec::new();
    let mut hash = String::new();
    let mut parent = String::new();
    let mut in_event = false;
    let mut expecting_value = false;

    for line in reader.lines() {
        let line = line?;

        if line == "---" {
            // Start of new event
            hash.clear();
            parent.clear();
            in_event = true;
            expecting_value = false;
            continue;
        }

        if !in_event {
            continue;
        }

        if let Some(h) = line.strip_prefix("hash: ") {
            hash = h.to_string();
            expecting_value = false;
        } else if let Some(p) = line.strip_prefix("parent: ") {
            parent = p.to_string();
            expecting_value = true; // next non-empty line is the value
        } else if expecting_value && !line.is_empty() {
            // This is the Value literal line
            let hash_bytes = hex::decode(&hash).unwrap_or_default();
            let parent_bytes = if parent == "root" {
                None
            } else {
                Some(hex::decode(&parent).unwrap_or_default())
            };

            // Parse the value literal and re-encode to CGRF
            let data = if line.starts_with("raw:") {
                vec![] // can't reconstruct raw data
            } else {
                match parse_value(&line) {
                    Ok(value) => packr_abi::encode(&value).unwrap_or_default(),
                    Err(_) => vec![],
                }
            };

            events.push(ChainEvent {
                hash: hash_bytes,
                parent_hash: parent_bytes,
                event_type: String::new(), // event_type is inside the Value
                data,
            });

            in_event = false;
            expecting_value = false;
        }
    }

    Ok(events)
}

/// Load a `.chain` file and reconstruct `ChainEvent`s for replay.
pub fn load_chain_file(path: &std::path::Path) -> io::Result<Vec<ChainEvent>> {
    let file = std::fs::File::open(path)?;
    let mut reader = io::BufReader::new(file);
    parse_events(&mut reader)
}
