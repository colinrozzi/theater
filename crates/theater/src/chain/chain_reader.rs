//! # Chain Reader
//!
//! Parses chain files in either EVENT format or JSON format back into `ChainEvent` objects.
//!
//! ## EVENT Format
//! ```text
//! EVENT <hash-hex>
//! <parent-hash-hex or 0000000000000000...>
//! <event-type>
//! <body-length>
//!
//! <body bytes>
//!
//! ```
//!
//! ## JSON Format
//! A JSON array of ChainEvent objects (as produced by serde_json).

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::chain::ChainEvent;

/// Reads and parses a chain file (EVENT or JSON format) into a vector of `ChainEvent`s.
pub struct ChainReader;

impl ChainReader {
    /// Parse a chain file from a path. Auto-detects EVENT vs JSON format.
    pub fn read_file(path: &Path) -> Result<Vec<ChainEvent>> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read chain file: {:?}", path))?;

        let trimmed = content.trim_start();

        // Auto-detect format: JSON starts with '[', EVENT format starts with 'EVENT'
        if trimmed.starts_with('[') {
            // JSON format
            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse JSON chain file: {:?}", path))
        } else {
            // EVENT format
            Self::read(BufReader::new(content.as_bytes()))
        }
    }

    /// Parse chain events from a reader.
    pub fn read<R: BufRead>(mut reader: R) -> Result<Vec<ChainEvent>> {
        let mut events = Vec::new();
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break; // EOF
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue; // Skip blank lines between events
            }

            // Expect "EVENT <hash>"
            if !trimmed.starts_with("EVENT ") {
                return Err(anyhow!("Expected 'EVENT <hash>', got: {}", trimmed));
            }

            let hash_hex = &trimmed[6..]; // Skip "EVENT "
            let hash = hex::decode(hash_hex)
                .with_context(|| format!("Invalid hash hex: {}", hash_hex))?;

            // Read parent hash line
            line.clear();
            reader.read_line(&mut line)?;
            let parent_hex = line.trim();
            let parent_hash = if parent_hex.chars().all(|c| c == '0') {
                None
            } else {
                Some(hex::decode(parent_hex)
                    .with_context(|| format!("Invalid parent hash hex: {}", parent_hex))?)
            };

            // Read event type line
            line.clear();
            reader.read_line(&mut line)?;
            let event_type = line.trim().to_string();

            // Read body length line
            line.clear();
            reader.read_line(&mut line)?;
            let body_len: usize = line.trim().parse()
                .with_context(|| format!("Invalid body length: {}", line.trim()))?;

            // Read blank line before body
            line.clear();
            reader.read_line(&mut line)?;

            // Read body bytes
            let mut data = vec![0u8; body_len];
            reader.read_exact(&mut data)
                .with_context(|| format!("Failed to read {} body bytes", body_len))?;

            // Read trailing newlines after body
            line.clear();
            reader.read_line(&mut line)?; // First newline after body
            // Second newline is handled by the loop

            events.push(ChainEvent {
                hash,
                parent_hash,
                event_type,
                data,
            });
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_single_event() {
        let chain_data = b"EVENT 1a2b3c
0000000000000000000000000000000000000000
test.event
11

hello world

";
        let events = ChainReader::read(BufReader::new(Cursor::new(chain_data))).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].hash, vec![0x1a, 0x2b, 0x3c]);
        assert_eq!(events[0].parent_hash, None);
        assert_eq!(events[0].event_type, "test.event");
        assert_eq!(events[0].data, b"hello world");
    }

    #[test]
    fn test_parse_chained_events() {
        let chain_data = b"EVENT aabbcc
0000000000000000000000000000000000000000
first.event
5

hello

EVENT ddeeff
aabbcc
second.event
5

world

";
        let events = ChainReader::read(BufReader::new(Cursor::new(chain_data))).unwrap();

        assert_eq!(events.len(), 2);

        assert_eq!(events[0].hash, vec![0xaa, 0xbb, 0xcc]);
        assert_eq!(events[0].parent_hash, None);
        assert_eq!(events[0].event_type, "first.event");

        assert_eq!(events[1].hash, vec![0xdd, 0xee, 0xff]);
        assert_eq!(events[1].parent_hash, Some(vec![0xaa, 0xbb, 0xcc]));
        assert_eq!(events[1].event_type, "second.event");
    }
}
