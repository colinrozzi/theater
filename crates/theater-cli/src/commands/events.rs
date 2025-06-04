use clap::Parser;
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::debug;

use theater::id::TheaterId;
use theater::chain::ChainEvent;
use crate::error::{CliError, CliResult};
use crate::output::formatters::ActorEvents;
use crate::CommandContext;

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

    /// Sort events (chain, time, type, size)
    #[arg(short, long, default_value = "chain")]
    pub sort: String,

    /// Reverse the sort order
    #[arg(short = 'r', long)]
    pub reverse: bool,

    /// Show detailed event information
    #[arg(short = 'd', long)]
    pub detailed: bool,
}

/// Execute the events command asynchronously with modern patterns
pub async fn execute_async(args: &EventsArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Getting events for actor: {}", args.actor_id);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id)
        .map_err(|_| CliError::InvalidInput {
            field: "actor_id".to_string(),
            value: args.actor_id.clone(),
            suggestion: "Provide a valid actor ID in the correct format".to_string(),
        })?;

    // Create client and connect
    let client = ctx.create_client();
    client.connect().await
        .map_err(|e| CliError::connection_failed(args.address, e))?;

    // Get the actor events
    let mut events = client.get_actor_events(&actor_id.to_string()).await
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to get actor events: {}", e),
        })?;

    // Apply filters
    apply_filters(&mut events, args)?;

    // Apply sorting
    apply_sorting(&mut events, &args.sort, args.reverse)?;

    // Limit the number of events if requested
    if args.limit > 0 && events.len() > args.limit {
        events = events.into_iter().take(args.limit).collect();
    }

    // Create formatted output
    let actor_events = ActorEvents {
        actor_id: actor_id.to_string(),
        events,
    };

    // Output using the configured format
    let format = if ctx.json { Some("json") } else { None };
    ctx.output.output(&actor_events, format)?;

    Ok(())
}

/// Apply various filters to the events
fn apply_filters(events: &mut Vec<ChainEvent>, args: &EventsArgs) -> CliResult<()> {
    // Filter by event type
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

    Ok(())
}

/// Apply sorting to the events
fn apply_sorting(events: &mut Vec<ChainEvent>, sort_type: &str, reverse: bool) -> CliResult<()> {
    match sort_type {
        "chain" => {
            let ordered_events = order_events_by_chain(events, reverse);
            *events = ordered_events;
        }
        "time" => {
            if reverse {
                events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            } else {
                events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            }
        }
        "type" => {
            if reverse {
                events.sort_by(|a, b| b.event_type.cmp(&a.event_type));
            } else {
                events.sort_by(|a, b| a.event_type.cmp(&b.event_type));
            }
        }
        "size" => {
            if reverse {
                events.sort_by(|a, b| a.data.len().cmp(&b.data.len()));
            } else {
                events.sort_by(|a, b| b.data.len().cmp(&a.data.len()));
            }
        }
        _ => {
            return Err(CliError::InvalidInput {
                field: "sort".to_string(),
                value: sort_type.to_string(),
                suggestion: "Use one of: chain, time, type, size".to_string(),
            });
        }
    }
    Ok(())
}

// Helper function to parse time specifications like "1h", "2d", or unix timestamps
fn parse_time_spec(spec: &str) -> CliResult<u64> {
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
        .map_err(|_| CliError::InvalidInput {
            field: "time".to_string(),
            value: spec.to_string(),
            suggestion: "Use format like '1h', '2d', '30m', or a unix timestamp".to_string(),
        })?;

    match unit.as_str() {
        "s" => Ok(now - amount),
        "m" => Ok(now - amount * 60),
        "h" => Ok(now - amount * 3600),
        "d" => Ok(now - amount * 86400),
        "w" => Ok(now - amount * 604800),
        _ => Err(CliError::InvalidInput {
            field: "time_unit".to_string(),
            value: unit,
            suggestion: "Use time units: s (seconds), m (minutes), h (hours), d (days), w (weeks)".to_string(),
        }),
    }
}

// Order events by their chain structure (parent-child relationships)
fn order_events_by_chain(events: &[ChainEvent], reverse: bool) -> Vec<ChainEvent> {
    if events.is_empty() {
        return Vec::new();
    }

    use std::collections::HashMap;

    // Find the root event (the one without a parent)
    let root = events.iter().find(|e| e.parent_hash.is_none());

    // If no root is found, return events as-is
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

// Keep the legacy function for backward compatibility
pub fn execute(args: &EventsArgs, verbose: bool, json: bool) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let output = crate::output::OutputManager::new(config.output.clone());
        
        let ctx = CommandContext {
            config,
            output,
            verbose,
            json,
        };
        
        execute_async(args, &ctx).await.map_err(Into::into)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::output::OutputManager;

    #[tokio::test]
    async fn test_events_command_invalid_actor_id() {
        let args = EventsArgs {
            actor_id: "invalid-id".to_string(),
            address: "127.0.0.1:9000".parse().unwrap(),
            limit: 0,
            event_type: None,
            from: None,
            to: None,
            search: None,
            sort: "chain".to_string(),
            reverse: false,
            detailed: false,
        };
        let config = Config::default();
        let output = OutputManager::new(config.output.clone());
        
        let ctx = CommandContext {
            config,
            output,
            verbose: false,
            json: false,
        };
        
        let result = execute_async(&args, &ctx).await;
        assert!(result.is_err());
        if let Err(CliError::InvalidInput { field, .. }) = result {
            assert_eq!(field, "actor_id");
        } else {
            panic!("Expected InvalidInput error");
        }
    }

    #[test]
    fn test_parse_time_spec() {
        // Test unix timestamp
        assert_eq!(parse_time_spec("1000").unwrap(), 1000);
        
        // Test relative times (will be based on current time, so just check they don't error)
        assert!(parse_time_spec("1h").is_ok());
        assert!(parse_time_spec("2d").is_ok());
        assert!(parse_time_spec("30m").is_ok());
        
        // Test invalid formats
        assert!(parse_time_spec("invalid").is_err());
        assert!(parse_time_spec("1x").is_err());
    }
}
