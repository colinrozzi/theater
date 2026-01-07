use anyhow::Result;
use console::style;
use std::collections::HashMap;
use theater::chain::ChainEvent;
use theater::id::TheaterId;

/// Structure for configuring event display options
pub struct EventDisplayOptions {
    pub format: String,
    pub detailed: bool,
    pub json: bool,
}

impl Default for EventDisplayOptions {
    fn default() -> Self {
        Self {
            format: "compact".to_string(),
            detailed: false,
            json: false,
        }
    }
}

/// Display a batch of events with formatting options
pub fn display_events(
    events: &[ChainEvent],
    actor_id: Option<&TheaterId>,
    options: &EventDisplayOptions,
    start_count: usize,
) -> Result<usize> {
    let mut count = start_count;

    // Use the effective format (JSON overrides format option)
    let effective_format = if options.json {
        "json"
    } else {
        &options.format
    };

    // Print the header for compact format if this is the first batch
    if effective_format == "compact" && start_count == 0 {
        if let Some(id) = actor_id {
            println!(
                "{} Events for actor: {}",
                style("i").blue().bold(),
                style(id.to_string()).cyan()
            );
        }

        println!(
            "{:<12} {:<12} {:<30} {}",
            "HASH", "PARENT", "EVENT TYPE", "DATA"
        );
        println!("{}", style("-".repeat(100)).dim());
    }

    // If no events, show a message and return
    if events.is_empty() && start_count == 0 {
        println!("  No events found.");
        return Ok(0);
    }

    // Display all events according to format
    for event in events {
        count += 1;
        display_single_event(event, effective_format)?;
    }

    Ok(count)
}

/// Display a single event with formatting options
pub fn display_single_event(event: &ChainEvent, format: &str) -> Result<()> {
    match format {
        "json" => {
            let output = serde_json::json!({
                "event": event,
            });

            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        "compact" => {
            // Format event hash
            let hash_str = hex::encode(&event.hash);
            let short_hash = if hash_str.len() > 8 {
                format!("{}..{}", &hash_str[0..4], &hash_str[hash_str.len() - 4..])
            } else {
                hash_str
            };

            // Format parent hash
            let parent_hash = match &event.parent_hash {
                Some(hash) => {
                    let parent_str = hex::encode(hash);
                    if parent_str.len() > 8 {
                        format!(
                            "{}..{}",
                            &parent_str[0..4],
                            &parent_str[parent_str.len() - 4..]
                        )
                    } else {
                        parent_str
                    }
                }
                None => "-".to_string(),
            };

            // Get event type with color based on category
            let colored_type = match event.event_type.split('.').next().unwrap_or("") {
                "http" => style(&event.event_type).cyan(),
                "filesystem" => style(&event.event_type).green(),
                "message" => style(&event.event_type).magenta(),
                "runtime" => style(&event.event_type).blue(),
                "error" => style(&event.event_type).red(),
                "wasm" => style(&event.event_type).yellow(),
                _ => style(&event.event_type).white(),
            };

            // Get data preview
            let data_preview = if let Ok(text) = std::str::from_utf8(&event.data) {
                if text.len() > 40 {
                    format!("{}...", &text[0..37])
                } else {
                    text.to_string()
                }
            } else {
                format!("{} bytes", event.data.len())
            };

            println!(
                "{:<12} {:<12} {:<30} {}",
                style(&short_hash).dim(),
                style(&parent_hash).dim(),
                colored_type,
                data_preview
            );
        }
        "csv" => {
            let event_type = &event.event_type;
            let hash = hex::encode(&event.hash);
            let parent_hash = event
                .parent_hash
                .as_ref()
                .map(|h| hex::encode(h))
                .unwrap_or_default();
            let data_size = event.data.len();

            println!("{},{},{},{}", event_type, hash, parent_hash, data_size);
        }
        "detailed" => {
            let hash_str = hex::encode(&event.hash);
            let parent_hash = match &event.parent_hash {
                Some(hash) => hex::encode(hash),
                None => "-".to_string(),
            };

            let colored_type = match event.event_type.split('.').next().unwrap_or("") {
                "http" => style(&event.event_type).cyan(),
                "filesystem" => style(&event.event_type).green(),
                "message" => style(&event.event_type).magenta(),
                "runtime" => style(&event.event_type).blue(),
                "error" => style(&event.event_type).red(),
                "wasm" => style(&event.event_type).yellow(),
                _ => style(&event.event_type).white(),
            };

            // Print detailed event info
            println!(
                "{} [{}] {}",
                style("EVENT").bold().blue(),
                hash_str,
                colored_type
            );

            println!("   Parent Hash: {}", parent_hash);
            println!("   Data Size: {} bytes", event.data.len());

            if let Ok(text) = std::str::from_utf8(&event.data) {
                println!("   Data: {}", text);
            } else {
                // Print hex dump if binary data
                println!("\nHex Dump:");
                print_hex_dump(&event.data, 16);
            }

            println!();
        }
        _ => {
            // pretty format (default)
            // Format event type with color based on category
            let colored_type = match event.event_type.split('.').next().unwrap_or("") {
                "http" => style(&event.event_type).cyan(),
                "filesystem" => style(&event.event_type).green(),
                "message" => style(&event.event_type).magenta(),
                "runtime" => style(&event.event_type).blue(),
                "error" => style(&event.event_type).red(),
                "wasm" => style(&event.event_type).yellow(),
                _ => style(&event.event_type).white(),
            };

            let short_hash = {
                let hash_str = hex::encode(&event.hash);
                if hash_str.len() > 8 {
                    format!("{}..{}", &hash_str[0..4], &hash_str[hash_str.len() - 4..])
                } else {
                    hash_str
                }
            };

            // Print basic event info
            println!(
                "{} [{}] {}",
                style("EVENT").bold().blue(),
                short_hash,
                colored_type
            );

            println!();
        }
    }

    Ok(())
}

pub fn display_events_header(format: &str) {
    match format {
        "compact" => {
            println!(
                "{:<12} {:<12} {:<30} {}",
                "HASH", "PARENT", "EVENT TYPE", "DATA"
            );
            println!("{}", style("-".repeat(100)).dim());
        }
        _ => {
            println!("{} Events:", style("i").blue().bold());
        }
    }
}

/// Display a timeline view of events (simplified without timestamps)
pub fn display_events_timeline(events: &[ChainEvent], actor_id: &TheaterId) -> Result<()> {
    println!(
        "{} Event chain for actor: {}",
        style("i").blue().bold(),
        style(actor_id.to_string()).cyan()
    );

    if events.is_empty() {
        println!("  No events found.");
        return Ok(());
    }

    // Get terminal width for display
    let term_width = match term_size::dimensions() {
        Some((w, _)) => w.min(120),
        None => 80,
    };

    println!("{}", style("-".repeat(term_width)).dim());

    // Group events by type for the summary
    let mut event_types: HashMap<&str, usize> = HashMap::new();
    for event in events {
        *event_types.entry(&event.event_type).or_insert(0) += 1;
    }

    // Display summary of event types
    println!("Event types:");
    for (event_type, count) in event_types.iter() {
        let type_color = match event_type.split('.').next().unwrap_or("") {
            "http" => style(event_type).cyan(),
            "filesystem" => style(event_type).green(),
            "message" => style(event_type).magenta(),
            "runtime" => style(event_type).blue(),
            "error" => style(event_type).red(),
            "wasm" => style(event_type).yellow(),
            _ => style(event_type).white(),
        };
        println!("  {} - {} events", type_color, count);
    }

    println!("{}", style("-".repeat(term_width)).dim());
    println!("Chain ({} events):", events.len());

    // Display chain sequence
    for (i, event) in events.iter().enumerate() {
        let hash_str = hex::encode(&event.hash);
        let short_hash = if hash_str.len() > 8 {
            format!("{}..{}", &hash_str[0..4], &hash_str[hash_str.len() - 4..])
        } else {
            hash_str
        };

        // Get event type with color
        let event_type = match event.event_type.split('.').next().unwrap_or("") {
            "http" => style(&event.event_type).cyan(),
            "filesystem" => style(&event.event_type).green(),
            "message" => style(&event.event_type).magenta(),
            "runtime" => style(&event.event_type).blue(),
            "error" => style(&event.event_type).red(),
            "wasm" => style(&event.event_type).yellow(),
            _ => style(&event.event_type).white(),
        };

        println!("  {:>4}. [{}] {}", i + 1, short_hash, event_type);
    }

    Ok(())
}

/// Create a CSV header row
pub fn display_csv_header() {
    println!("event_type,hash,parent_hash,data_size");
}

/// Helper function to pretty-stringify an event for the pretty format
pub fn pretty_stringify_event(event: &ChainEvent, full: bool) -> String {
    let event_type = match event.event_type.split('.').next().unwrap_or("") {
        "http" => style(&event.event_type).cyan(),
        "filesystem" => style(&event.event_type).green(),
        "message" => style(&event.event_type).magenta(),
        "runtime" => style(&event.event_type).blue(),
        "error" => style(&event.event_type).red(),
        "wasm" => style(&event.event_type).yellow(),
        _ => style(&event.event_type).white(),
    };

    let hash_str = hex::encode(&event.hash);
    let short_hash = if hash_str.len() > 8 {
        format!("{}..{}", &hash_str[0..4], &hash_str[hash_str.len() - 4..])
    } else {
        hash_str
    };

    let mut output = format!(
        "{} [{}] [{}]\n",
        style(">").bold().blue(),
        event_type,
        short_hash,
    );

    if let Some(parent) = &event.parent_hash {
        let parent_str = hex::encode(parent);
        let short_parent = if parent_str.len() > 8 {
            format!(
                "{}..{}",
                &parent_str[0..4],
                &parent_str[parent_str.len() - 4..]
            )
        } else {
            parent_str
        };
        output.push_str(&format!("  Parent: {}\n", short_parent));
    }

    if let Ok(text) = std::str::from_utf8(&event.data) {
        if !full {
            let max_len = std::cmp::min(text.len(), 57);
            output.push_str(&format!(
                "  Data: {}... ({} bytes total)\n",
                &text[0..max_len],
                event.data.len()
            ));
        } else {
            output.push_str(&format!("  Data: {}\n", text));
        }
    } else if !event.data.is_empty() {
        output.push_str(&format!(
            "  Data: {} bytes of binary data\n",
            event.data.len()
        ));
    }

    output.push('\n');
    output
}

/// Helper function to print a hex dump of binary data
#[allow(dead_code)]
pub fn print_hex_dump(data: &[u8], bytes_per_line: usize) {
    let mut offset = 0;
    while offset < data.len() {
        let bytes_to_print = std::cmp::min(bytes_per_line, data.len() - offset);
        let line_data = &data[offset..offset + bytes_to_print];

        // Print the offset
        print!("    {:08x}  ", offset);

        // Print hex values
        for (i, byte) in line_data.iter().enumerate() {
            print!("{:02x} ", byte);
            if i == 7 {
                print!(" "); // Extra space at the middle
            }
        }

        // Pad with spaces if we don't have enough bytes
        if bytes_to_print < bytes_per_line {
            for _ in 0..(bytes_per_line - bytes_to_print) {
                print!("   ");
            }
            // Extra space if we're missing the middle marker
            if bytes_to_print <= 7 {
                print!(" ");
            }
        }

        // Print ASCII representation
        print!(" |");
        for byte in line_data {
            if *byte >= 32 && *byte <= 126 {
                // Printable ASCII
                print!("{}", *byte as char);
            } else {
                // Non-printable
                print!(".");
            }
        }
        println!("|");

        offset += bytes_per_line;
    }
}

/// Display a single event in structured format for Unix scripting
pub fn display_structured_event(event: &ChainEvent, fields: &[&str]) -> Result<()> {
    // Start with EVENT and hash (always included)
    let hash_str = format!("0x{}", hex::encode(&event.hash));
    println!("EVENT {}", hash_str);

    // Helper to check if field should be included
    let should_include = |field: &str| fields.contains(&field);

    // Parent hash (if requested and exists)
    if should_include("parent") {
        match &event.parent_hash {
            Some(parent) => println!("0x{}", hex::encode(parent)),
            None => println!("0x0000000000000000"),
        }
    }

    // Event type (if requested)
    if should_include("type") {
        println!("{}", event.event_type);
    }

    // Data size (if requested)
    if should_include("data_size") {
        println!("{}", event.data.len());
    }

    // Empty line before data
    println!();

    // Data (if requested)
    if should_include("data") {
        if let Ok(text) = std::str::from_utf8(&event.data) {
            print!("{}", text);
        } else if !event.data.is_empty() {
            // For binary data, output as hex
            print!("{}", hex::encode(&event.data));
        }
    }

    // End with separator
    println!("\n\n");

    Ok(())
}

/// Parse event fields from comma-separated string
pub fn parse_event_fields(fields_str: &str) -> Vec<&str> {
    fields_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        // Filter out unsupported fields
        .filter(|s| *s != "timestamp" && *s != "description")
        .collect()
}
