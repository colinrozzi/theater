use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;

use tracing::debug;

use crate::cli::client::TheaterClient;
use theater::chain::ChainEvent;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct EventsArgs {
    /// ID of the actor to get events from
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Number of events to show (0 for all)
    #[arg(short, long, default_value = "0")]
    pub limit: usize,

    /// Filter events by type (e.g., http.request, runtime.init)
    #[arg(short = 't', long)]
    pub event_type: Option<String>,

    /// Show events from this timestamp onward (Unix timestamp or relative time like "1h", "2d")
    #[arg(long)]
    pub from: Option<String>,

    /// Show events until this timestamp (Unix timestamp or relative time like "1h", "2d")
    #[arg(long)]
    pub to: Option<String>,

    /// Search events for this text (in description and data)
    #[arg(long)]
    pub search: Option<String>,

    /// Output format (pretty, compact, json, csv)
    #[arg(short, long, default_value = "compact")]
    pub format: String,

    /// Export events to file
    #[arg(short, long)]
    pub export: Option<String>,

    /// Show detailed event information
    #[arg(short = 'd', long)]
    pub detailed: bool,

    /// Sort events (chain, time, type, size)
    #[arg(short, long, default_value = "chain")]
    pub sort: String,

    /// Reverse the sort order
    #[arg(short = 'r', long)]
    pub reverse: bool,

    /// Show events in a timeline view
    #[arg(long)]
    pub timeline: bool,
}

pub fn execute(args: &EventsArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Getting events for actor: {}", args.actor_id);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id)
        .map_err(|_| anyhow!("Invalid actor ID: {}", args.actor_id))?;

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);

        // Connect to the server
        client.connect().await?;

        // Get the actor events
        let mut events = client.get_actor_events(actor_id.clone()).await?;

        // Apply filters
        if let Some(event_type) = &args.event_type {
            events.retain(|e| e.event_type.contains(event_type));
        }

        // Parse and apply timestamp filters
        if let Some(from_str) = &args.from {
            let from_time = parse_time_spec(from_str)?;
            events.retain(|e| e.timestamp >= from_time);
        }

        if let Some(to_str) = &args.to {
            let to_time = parse_time_spec(to_str)?;
            events.retain(|e| e.timestamp <= to_time);
        }

        // Apply text search
        if let Some(search_text) = &args.search {
            events.retain(|e| {
                // Search in event type
                if e.event_type.contains(search_text) {
                    return true;
                }

                // Search in description
                if let Some(desc) = &e.description {
                    if desc.contains(search_text) {
                        return true;
                    }
                }

                // Search in data if it's UTF-8 text
                if let Ok(data_str) = std::str::from_utf8(&e.data) {
                    if data_str.contains(search_text) {
                        return true;
                    }
                }

                false
            });
        }

        // Organize events by chain structure if "chain" sort is requested, otherwise use standard sorts
        match args.sort.as_str() {
            "chain" => {
                // Sort events by their chain structure (parent-child relationships)
                let ordered_events = order_events_by_chain(&events, args.reverse);
                events = ordered_events;
            }
            "time" => {
                if args.reverse {
                    events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                } else {
                    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
                }
            }
            "type" => {
                if args.reverse {
                    events.sort_by(|a, b| b.event_type.cmp(&a.event_type));
                } else {
                    events.sort_by(|a, b| a.event_type.cmp(&b.event_type));
                }
            }
            "size" => {
                if args.reverse {
                    events.sort_by(|a, b| a.data.len().cmp(&b.data.len()));
                } else {
                    events.sort_by(|a, b| b.data.len().cmp(&a.data.len()));
                }
            }
            _ => {
                // Default to chain
                let ordered_events = order_events_by_chain(&events, args.reverse);
                events = ordered_events;
            }
        }

        // Limit the number of events if requested
        if args.limit > 0 && events.len() > args.limit {
            events = events.into_iter().take(args.limit).collect();
        }

        // Handle export if requested
        if let Some(export_path) = &args.export {
            export_events(&events, export_path, &args.format)?;
            println!(
                "{} Exported {} events to {}",
                style("✓").green().bold(),
                events.len(),
                export_path
            );
        }

        // Display events according to format or timeline view
        if args.timeline {
            display_events_timeline(&events, &actor_id)?
        } else {
            // If JSON flag is set, it overrides the format option
            let output_format = if json { "json" } else { &args.format };

            match output_format {
                "json" => display_events_json(&events, &actor_id)?,
                "csv" => display_events_csv(&events)?,
                "compact" => display_events_compact(&events, &actor_id)?,
                _ => display_events_pretty(&events, &actor_id, args.detailed, args.limit)?,
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

// Helper function to parse time specifications like "1h", "2d", or unix timestamps
fn parse_time_spec(spec: &str) -> Result<u64> {
    // Try parsing as a simple timestamp first
    if let Ok(timestamp) = spec.parse::<u64>() {
        return Ok(timestamp);
    }

    // Try parsing as a relative time
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let (amount_str, unit) = spec.chars().partition::<String, _>(|c| c.is_ascii_digit());
    let amount = amount_str
        .parse::<u64>()
        .map_err(|_| anyhow!("Invalid time specification: {}", spec))?;

    match unit.as_str() {
        "s" => Ok(now - amount),
        "m" => Ok(now - amount * 60),
        "h" => Ok(now - amount * 3600),
        "d" => Ok(now - amount * 86400),
        "w" => Ok(now - amount * 604800),
        _ => Err(anyhow!("Unknown time unit: {}", unit)),
    }
}

// Export events to a file
fn export_events(events: &[ChainEvent], path: &str, format: &str) -> Result<()> {
    let content = match format {
        "json" => serde_json::to_string_pretty(events)?,
        "csv" => {
            let mut wtr = csv::Writer::from_writer(vec![]);

            // Write header
            wtr.write_record(&[
                "timestamp",
                "event_type",
                "hash",
                "parent_hash",
                "description",
                "data_size",
            ])?;

            // Write data
            for event in events {
                let hash_hex = hex::encode(&event.hash);
                let parent_hash_hex = event
                    .parent_hash
                    .as_ref()
                    .map(|h| hex::encode(h))
                    .unwrap_or_else(|| String::from(""));

                wtr.write_record(&[
                    &event.timestamp.to_string(),
                    &event.event_type,
                    &hash_hex,
                    &parent_hash_hex,
                    &event
                        .description
                        .clone()
                        .unwrap_or_else(|| String::from("")),
                    &event.data.len().to_string(),
                ])?;
            }

            String::from_utf8(wtr.into_inner()?)?
        }
        "pretty" => {
            let mut output = String::new();
            for event in events {
                output.push_str(&pretty_stringify_event(event, true));
                output.push_str("\n");
            }
            output
        }
        _ => serde_json::to_string_pretty(events)?, // Default to JSON
    };

    std::fs::write(path, content)?;
    Ok(())
}

// Display events in JSON format
fn display_events_json(events: &[ChainEvent], actor_id: &TheaterId) -> Result<()> {
    let output = serde_json::json!({
        "actor_id": actor_id.to_string(),
        "events": events,
        "count": events.len()
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

// Display events in CSV format
fn display_events_csv(events: &[ChainEvent]) -> Result<()> {
    println!("timestamp,event_type,hash,parent_hash,description,data_size");

    for event in events {
        let timestamp = event.timestamp;
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

    Ok(())
}

// Display events in compact format
fn display_events_compact(events: &[ChainEvent], actor_id: &TheaterId) -> Result<()> {
    println!(
        "{} Events for actor: {}",
        style("ℹ").blue().bold(),
        style(actor_id.to_string()).cyan()
    );

    if events.is_empty() {
        println!("  No events found.");
        return Ok(());
    }

    println!(
        "{:<5} {:<19} {:<30} {}",
        "#", "TIMESTAMP", "EVENT TYPE", "DESCRIPTION"
    );
    println!("{}", style("─".repeat(80)).dim());

    for (i, event) in events.iter().enumerate() {
        // Format timestamp as human-readable
        let timestamp = chrono::DateTime::from_timestamp(event.timestamp as i64, 0)
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        // Get event type with color based on category
        let event_type = match event.event_type.split('.').next().unwrap_or("") {
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
            "{:<5} {:<19} {:<30} {}",
            i + 1,
            timestamp,
            event_type,
            description
        );
    }

    Ok(())
}

// Display events in pretty format
fn display_events_pretty(
    events: &[ChainEvent],
    actor_id: &TheaterId,
    detailed: bool,
    limit: usize,
) -> Result<()> {
    println!(
        "{} Events for actor: {}",
        style("ℹ").blue().bold(),
        style(actor_id.to_string()).cyan()
    );

    if events.is_empty() {
        println!("  No events found.");
        return Ok(());
    }

    for (i, event) in events.iter().enumerate() {
        println!(
            "{}",
            pretty_stringify_event(event, detailed).replace("\n", "\n  ")
        );

        if i < events.len() - 1 {
            println!("{}", style("─".repeat(80)).dim());
        }
    }

    if limit > 0 && events.len() == limit {
        println!(
            "(Showing {} of many events. Use --limit to see more.)",
            events.len()
        );
    }

    Ok(())
}

fn pretty_stringify_event(event: &ChainEvent, full: bool) -> String {
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

// Display events in timeline view
fn display_events_timeline(events: &[ChainEvent], actor_id: &TheaterId) -> Result<()> {
    println!(
        "{} Timeline for actor: {}",
        style("ℹ").blue().bold(),
        style(actor_id.to_string()).cyan()
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

// Order events by their chain structure (parent-child relationships)
// We are guaranteed that we are given all events in a chain, and that there are no cycles, and
// that there is always exactly one root event. All events have only one parent, except the root.
// Only one child event can have a given parent.
fn order_events_by_chain(events: &[ChainEvent], reverse: bool) -> Vec<ChainEvent> {
    if events.is_empty() {
        return Vec::new();
    }

    // Find the root event (the one without a parent)
    let root = events.iter().find(|e| e.parent_hash.is_none());

    // If no root is found (should not happen with given guarantees), return events as-is
    let root = match root {
        Some(r) => r,
        None => return events.to_vec(),
    };

    // Create a map from parent hash to children
    let mut parent_to_children: HashMap<Vec<u8>, Vec<&ChainEvent>> = HashMap::new();
    for event in events {
        if let Some(parent_hash) = &event.parent_hash {
            parent_to_children
                .entry(parent_hash.clone())
                .or_insert_with(Vec::new)
                .push(event);
        }
    }

    // Function to recursively collect events in order
    let mut ordered_events = Vec::new();

    fn traverse_chain(
        event: &ChainEvent,
        parent_to_children: &HashMap<Vec<u8>, Vec<&ChainEvent>>,
        ordered_events: &mut Vec<ChainEvent>,
    ) {
        ordered_events.push(event.clone());

        if let Some(children) = parent_to_children.get(&event.hash) {
            for &child in children {
                traverse_chain(child, parent_to_children, ordered_events);
            }
        }
    }

    // Start traversal from the root
    traverse_chain(root, &parent_to_children, &mut ordered_events);

    // Reverse if requested
    if reverse {
        ordered_events.reverse();
    }

    ordered_events
}

// Helper function to format timestamps in a human-readable way
fn format_timestamp(timestamp: u64) -> String {
    chrono::DateTime::from_timestamp(timestamp as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

// Helper function to print a hex dump of binary data
#[allow(dead_code)]
fn print_hex_dump(data: &[u8], bytes_per_line: usize) {
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
