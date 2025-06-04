use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::str::FromStr;

use tracing::debug;

use crate::client::TheaterClient;
use crate::utils::event_display::{
    display_csv_header, display_events, display_events_timeline, pretty_stringify_event,
    EventDisplayOptions,
};
use theater::chain::ChainEvent;
use theater::id::TheaterId;

/// Get events for an actor (falls back to filesystem if actor is not running)
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
        let config = crate::config::Config::default();
        let mut client = TheaterClient::new(args.address, config);

        // Try to connect to the server, but continue even if it fails
        // (we'll automatically fall back to filesystem if needed)
        let connected = client.connect().await.is_ok();
        if !connected {
            debug!("Failed to connect to server, will attempt to read events from filesystem");
        }

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

        // Create display options
        let display_options = EventDisplayOptions {
            format: args.format.clone(),
            detailed: args.detailed,
            json,
        };

        // Display events according to format or timeline view
        if args.timeline {
            display_events_timeline(&events, &actor_id)?
        } else {
            // If JSON flag is set, it overrides the format option
            let format = if json {
                "json".to_string()
            } else {
                args.format.clone()
            };

            // Special handling for CSV format
            if format == "csv" {
                display_csv_header();
                display_events(&events, Some(&actor_id), &display_options, 0)?;
            } else {
                display_events(&events, Some(&actor_id), &display_options, 0)?;
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

// Order events by their chain structure (parent-child relationships)
// We are guaranteed that we are given all events in a chain, and that there are no cycles, and
// that there is always exactly one root event. All events have only one parent, except the root.
// Only one child event can have a given parent.
fn order_events_by_chain(events: &[ChainEvent], reverse: bool) -> Vec<ChainEvent> {
    if events.is_empty() {
        return Vec::new();
    }

    use std::collections::HashMap;

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

