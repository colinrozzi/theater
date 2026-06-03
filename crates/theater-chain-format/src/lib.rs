//! Lossless human-readable chain format for Theater `ChainEvent`s.
//!
//! Each event is three lines after the `---` separator:
//! 1. `hash: <full hex>`
//! 2. `parent: <full hex>` or `parent: root`
//! 3. The Pack Value literal of the entire event payload (one line)
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
//! The Pack Value literal is the canonical text form of the event's CGRF
//! payload — parsing it and re-encoding to CGRF round-trips back to the
//! exact bytes that produced the original hash, so a parsed chain file
//! verifies under [`theater_chain::ChainEvent`]'s hashing rules.
//!
//! The Theater runtime no longer emits chain files directly; this format is
//! preserved as a reusable utility for subscribers (replay actors, audit
//! tools, debug tails) that want to persist events in a human-inspectable
//! shape rather than opaque JSON.

use std::fmt::Write as _;
use std::io::{self, BufRead};

use packr_abi::parse_value;
use theater_chain::ChainEvent;

/// Format a chain event as a lossless EVENT block (terminated with a newline).
pub fn format_event(event: &ChainEvent) -> String {
    let mut out = String::new();
    writeln!(out, "---").unwrap();
    writeln!(out, "hash: {}", hex::encode(&event.hash)).unwrap();

    match &event.parent_hash {
        Some(parent) => writeln!(out, "parent: {}", hex::encode(parent)).unwrap(),
        None => writeln!(out, "parent: root").unwrap(),
    }

    if let Ok(value) = packr::decode(&event.data) {
        writeln!(out, "{}", value).unwrap();
    } else if !event.data.is_empty() {
        writeln!(out, "raw: {} bytes", event.data.len()).unwrap();
    }

    out
}

/// Format a sequence of events as a single chain file.
pub fn format_chain(events: &[ChainEvent]) -> String {
    events.iter().map(format_event).collect::<String>()
}

/// Parse an EVENT-block stream back into [`ChainEvent`]s.
///
/// Each block starts with `---`, followed by `hash:` and `parent:` lines,
/// then a Pack Value literal line that is re-encoded to CGRF to populate
/// `data`. Hash verification is the caller's responsibility — this function
/// only reconstructs the structure.
pub fn parse_events(reader: &mut dyn BufRead) -> io::Result<Vec<ChainEvent>> {
    let mut events = Vec::new();
    let mut hash = String::new();
    let mut parent = String::new();
    let mut data: Option<Vec<u8>> = None;
    let mut in_event = false;
    let mut expecting_value = false;

    let finalize =
        |events: &mut Vec<ChainEvent>, hash: &str, parent: &str, data: Option<Vec<u8>>| {
            if hash.is_empty() {
                return;
            }
            let hash_bytes = hex::decode(hash).unwrap_or_default();
            let parent_bytes = if parent == "root" {
                None
            } else {
                Some(hex::decode(parent).unwrap_or_default())
            };
            events.push(ChainEvent {
                hash: hash_bytes,
                parent_hash: parent_bytes,
                event_type: String::new(),
                data: data.unwrap_or_default(),
            });
        };

    for line in reader.lines() {
        let line = line?;

        if line == "---" {
            if in_event {
                finalize(&mut events, &hash, &parent, data.take());
            }
            hash.clear();
            parent.clear();
            data = None;
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
            expecting_value = true;
        } else if expecting_value && !line.is_empty() {
            data = Some(if line.starts_with("raw:") {
                vec![]
            } else {
                match parse_value(&line) {
                    Ok(value) => packr::encode(&value).unwrap_or_default(),
                    Err(_) => vec![],
                }
            });
            expecting_value = false;
        }
    }

    if in_event {
        finalize(&mut events, &hash, &parent, data.take());
    }

    Ok(events)
}

/// Load a `.chain` file and reconstruct the events.
pub fn load_chain_file(path: &std::path::Path) -> io::Result<Vec<ChainEvent>> {
    let file = std::fs::File::open(path)?;
    let mut reader = io::BufReader::new(file);
    parse_events(&mut reader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn root_event_has_parent_root() {
        let event = ChainEvent {
            hash: vec![0xab, 0xcd],
            parent_hash: None,
            event_type: String::new(),
            data: vec![],
        };
        let formatted = format_event(&event);
        assert!(formatted.contains("hash: abcd"));
        assert!(formatted.contains("parent: root"));
    }

    #[test]
    fn child_event_carries_parent_hex() {
        let event = ChainEvent {
            hash: vec![0x01, 0x23],
            parent_hash: Some(vec![0xab, 0xcd]),
            event_type: String::new(),
            data: vec![],
        };
        let formatted = format_event(&event);
        assert!(formatted.contains("hash: 0123"));
        assert!(formatted.contains("parent: abcd"));
    }

    #[test]
    fn format_then_parse_preserves_hash_and_parent_links() {
        let events = vec![
            ChainEvent {
                hash: vec![0xaa; 20],
                parent_hash: None,
                event_type: String::new(),
                data: vec![],
            },
            ChainEvent {
                hash: vec![0xbb; 20],
                parent_hash: Some(vec![0xaa; 20]),
                event_type: String::new(),
                data: vec![],
            },
        ];
        let formatted = format_chain(&events);
        let mut cursor = Cursor::new(formatted.into_bytes());
        let parsed = parse_events(&mut cursor).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].hash, vec![0xaa; 20]);
        assert!(parsed[0].parent_hash.is_none());
        assert_eq!(parsed[1].hash, vec![0xbb; 20]);
        assert_eq!(
            parsed[1].parent_hash.as_deref(),
            Some(vec![0xaa; 20].as_slice())
        );
    }
}
