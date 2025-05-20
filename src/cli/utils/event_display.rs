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
                style("ℹ").blue().bold(),
                style(id.to_string()).cyan()
            );
        }

        println!(
            "{:<12} {:<12} {:<25} {}",
            "HASH", "PARENT", "EVENT TYPE", "DESCRIPTION"
        );
        println!("{}", style("─".repeat(100)).dim());
    }

    // If no events, show a message and return
    if events.is_empty() && start_count == 0 {
        println!("  No events found.");
        return Ok(0);
    }

    // Display all events according to format
    for event in events {
        count += 1;
        display_single_event(event, effective_format, options.detailed)?;
    }

    Ok(count)
}

/// Display a single event with formatting options
pub fn display_single_event(event: &ChainEvent, format: &str, detailed: bool) -> Result<()> {
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
                _ => style(&event.event_type).yellow(),
            };

            // Get a concise description
            let description = event.description.clone().unwrap_or_else(|| {
                if let Ok(text) = std::str::from_utf8(&event.data) {
                    if text.len() > 40 {
                        format!("{}...", &text[0..37])
                    } else {
                        text.to_string()
                    }
                } else {
                    format!("{} bytes", event.data.len())
                }
            });

            println!(
                "{:<12} {:<12} {:<25} {}",
                style(&short_hash).dim(),
                style(&parent_hash).dim(),
                colored_type,
                description
            );
        }
        "csv" => {
            // Format timestamp
            let timestamp = event.timestamp.to_string();
            let event_type = &event.event_type;
            let hash = hex::encode(&event.hash);
            let parent_hash = event
                .parent_hash
                .as_ref()
                .map(|h| hex::encode(h))
                .unwrap_or_else(|| String::from(""));
            let description = event
                .description
                .as_deref()
                .unwrap_or("")
                .replace(',', "\\,"); // Escape commas in the description
            let data_size = event.data.len();

            println!(
                "{},{},{},{},{},{}",
                timestamp, event_type, hash, parent_hash, description, data_size
            );
        }
        _ => {
            // pretty format (default)
            // Format timestamp
            let timestamp = chrono::DateTime::from_timestamp(event.timestamp as i64, 0)
                .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH)
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string();

            // Format event type with color based on category
            let colored_type = match event.event_type.split('.').next().unwrap_or("") {
                "http" => style(&event.event_type).cyan(),
                "filesystem" => style(&event.event_type).green(),
                "message" => style(&event.event_type).magenta(),
                "runtime" => style(&event.event_type).blue(),
                "error" => style(&event.event_type).red(),
                _ => style(&event.event_type).yellow(),
            };

            // Print basic event info
            println!(
                "{} [{}] {}",
                style("EVENT").bold().blue(),
                timestamp,
                colored_type
            );

            // Show event description if available
            if let Some(desc) = &event.description {
                println!("   {}", desc);
            }

            // Show additional details if requested
            if detailed {
                println!("   Hash: {}", hex::encode(&event.hash));

                if let Some(parent) = &event.parent_hash {
                    println!("   Parent: {}", hex::encode(parent));
                }

                // Try to pretty-print the event data as JSON
                if let Ok(text) = std::str::from_utf8(&event.data) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                        println!("   Data: {}", serde_json::to_string_pretty(&json)?);
                    } else {
                        println!("   Data: {}", text);
                    }
                } else {
                    println!("   Data: {} bytes of binary data", event.data.len());
                }
            }

            println!("");
        }
    }

    Ok(())
}

pub fn display_events_header(format: &str) {
    match format {
        "compact" => {
            println!(
                "{:<12} {:<12} {:<25} {}",
                "HASH", "PARENT", "EVENT TYPE", "DESCRIPTION"
            );
            println!("{}", style("─".repeat(100)).dim());
        }
        _ => {
            println!("{} Events:", style("ℹ").blue().bold());
        }
    }
}

/// Display a timeline view of events
pub fn display_events_timeline(events: &[ChainEvent], actor_id: &TheaterId) -> Result<()> {
    println!(
        "{} Timeline for actor: {} {}",
        style("ℹ").blue().bold(),
        style(actor_id.to_string()).cyan(),
        style("")
    );

    if events.is_empty() {
        println!("  No events found.");
        return Ok(());
    }

    // Get the time range
    let start_time = events.iter().map(|e| e.timestamp).min().unwrap_or(0);
    let end_time = events.iter().map(|e| e.timestamp).max().unwrap_or(0);
    let range_ms = end_time.saturating_sub(start_time) as f64;

    // Get terminal width for timeline display
    let term_width = match term_size::dimensions() {
        Some((w, _)) => (w as f64 * 0.7) as usize, // Use 70% of terminal width for timeline
        None => 80, // Default to 80 if terminal size can't be determined
    };

    println!(
        "\nTime span: {} to {} ({} sec)",
        format_timestamp(start_time),
        format_timestamp(end_time),
        (range_ms / 1000.0).round()
    );

    println!("{}", style("─".repeat(term_width)).dim());

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
            _ => style(event_type).yellow(),
        };
        println!("  {} - {} events", type_color, count);
    }

    println!("{}", style("─".repeat(term_width)).dim());
    println!("Timeline:");

    // Display timeline for each event
    for event in events {
        // Calculate position on the timeline
        let position = if range_ms > 0.0 {
            ((event.timestamp - start_time) as f64 / range_ms * (term_width as f64 - 10.0)) as usize
        } else {
            0
        };

        // Format timestamp
        let time_str = chrono::DateTime::from_timestamp(event.timestamp as i64, 0)
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH)
            .format("%H:%M:%S")
            .to_string();

        // Get event type with color
        let event_type = match event.event_type.split('.').next().unwrap_or("") {
            "http" => style(&event.event_type).cyan(),
            "filesystem" => style(&event.event_type).green(),
            "message" => style(&event.event_type).magenta(),
            "runtime" => style(&event.event_type).blue(),
            "error" => style(&event.event_type).red(),
            _ => style(&event.event_type).yellow(),
        };

        // Print timeline with marker
        print!("[{}] {} ", time_str, event_type);

        // Display the timeline
        for i in 0..term_width - 10 {
            if i == position {
                print!("{}", style("●").bold());
            } else if i % 5 == 0 {
                print!("{}", style(".").dim());
            } else {
                print!(" ");
            }
        }
        println!("");

        // Print event description if available
        if let Some(desc) = &event.description {
            let trimmed_desc = if desc.len() > 60 {
                format!("{}...", &desc[0..57])
            } else {
                desc.clone()
            };
            println!("  {}", trimmed_desc);
        }
    }

    Ok(())
}

// Helper function to format timestamps in a human-readable way
pub fn format_timestamp(timestamp: u64) -> String {
    chrono::DateTime::from_timestamp(timestamp as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Create a CSV header row
pub fn display_csv_header() {
    println!("timestamp,event_type,hash,parent_hash,description,data_size");
}

pub fn display_compact_header() {
    println!(
        "{:<12} {:<12} {:<25} {}",
        "HASH", "PARENT", "EVENT TYPE", "DESCRIPTION"
    );
    println!("{}", style("─".repeat(100)).dim());
}

/// Helper function to pretty-stringify an event for the pretty format
pub fn pretty_stringify_event(event: &ChainEvent, full: bool) -> String {
    let timestamp = chrono::DateTime::from_timestamp(event.timestamp as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH)
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string();

    let event_type = match event.event_type.split('.').next().unwrap_or("") {
        "http" => style(&event.event_type).cyan(),
        "filesystem" => style(&event.event_type).green(),
        "message" => style(&event.event_type).magenta(),
        "runtime" => style(&event.event_type).blue(),
        "error" => style(&event.event_type).red(),
        _ => style(&event.event_type).yellow(),
    };

    let mut output = format!(
        "{} [{}] [{}]\n",
        style("►").bold().blue(),
        event_type,
        timestamp,
    );

    let hash_str = hex::encode(&event.hash);
    let short_hash = if hash_str.len() > 8 {
        format!("{}..{}", &hash_str[0..4], &hash_str[hash_str.len() - 4..])
    } else {
        hash_str
    };
    output.push_str(&format!("  Hash: {}\n", short_hash));

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

    if let Some(desc) = &event.description {
        output.push_str(&format!("  Description: {}\n", desc));
    }

    if let Ok(text) = std::str::from_utf8(&event.data) {
        if !full {
            // Do either the 57 chars or the max length of the string
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

    output.push_str("\n");
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
