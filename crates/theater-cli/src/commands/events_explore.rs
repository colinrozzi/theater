use clap::Parser;
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::tui::event_explorer;
use crate::CommandContext;
use theater::chain::ChainEvent;
use theater::id::TheaterId;

/// Interactively explore actor events with a rich TUI interface
#[derive(Debug, Parser)]
pub struct ExploreArgs {
    /// ID of the actor to explore events for
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Connect to running actor for live events
    #[arg(short, long)]
    pub live: bool,

    /// Start in follow mode (auto-scroll new events)
    #[arg(short, long)]
    pub follow: bool,

    /// Number of events to load initially (0 for all)
    #[arg(short = 'n', long, default_value = "1000")]
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
}

/// Execute the events explore command asynchronously
pub async fn execute_async(args: &ExploreArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Starting event explorer for actor: {}", args.actor_id);
    debug!("Server address: {}", args.address);
    debug!("Live mode: {}", args.live);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id).map_err(|_| CliError::InvalidInput {
        field: "actor_id".to_string(),
        value: args.actor_id.clone(),
        suggestion: "Provide a valid actor ID in the correct format".to_string(),
    })?;

    // Load initial events
    let events = load_events(args, ctx, &actor_id).await?;

    // Create and configure the explorer app
    let mut app = event_explorer::EventExplorerApp::new(
        args.actor_id.clone(),
        args.live,
        args.follow,
    );
    
    // Apply initial filters if specified
    if let Some(event_type) = &args.event_type {
        app.set_event_type_filter(event_type.clone());
    }
    
    if let Some(search) = &args.search {
        app.set_search_query(search.clone());
    }

    // Load events into the app
    app.load_events(events);

    // Launch the TUI
    event_explorer::run_explorer(app, ctx, args.address).await?;

    Ok(())
}

/// Load events from either live actor or stored data
async fn load_events(
    args: &ExploreArgs,
    ctx: &CommandContext,
    actor_id: &TheaterId,
) -> CliResult<Vec<ChainEvent>> {
    debug!("Loading events for actor: {}", actor_id);

    // Create client and connect
    let client = ctx.create_client();
    
    if args.live {
        // Try to connect to live actor first
        match client.connect().await {
            Ok(_) => {
                debug!("Connected to live actor");
                // Get events from running actor
                client
                    .get_actor_events(&actor_id.to_string())
                    .await
                    .map_err(|e| CliError::ServerError {
                        message: format!("Failed to get live actor events: {}", e),
                    })
            }
            Err(e) => {
                debug!("Failed to connect to live actor: {}, falling back to stored events", e);
                // Fall back to stored events
                load_stored_events(args, ctx, actor_id).await
            }
        }
    } else {
        // Load from stored events
        load_stored_events(args, ctx, actor_id).await
    }
}

/// Load events from filesystem/stored data
async fn load_stored_events(
    args: &ExploreArgs,
    ctx: &CommandContext,
    actor_id: &TheaterId,
) -> CliResult<Vec<ChainEvent>> {
    debug!("Loading stored events for actor: {}", actor_id);

    // Create a temporary EventsArgs to reuse existing filtering logic
    let events_args = crate::commands::events::EventsArgs {
        actor_id: actor_id.to_string(),
        address: args.address,
        limit: args.limit,
        event_type: args.event_type.clone(),
        from: args.from.clone(),
        to: args.to.clone(),
        search: args.search.clone(),
        sort: "chain".to_string(),
        reverse: false,
        detailed: false,
    };

    // Use existing events command logic to load and filter events
    let client = ctx.create_client();
    client
        .connect()
        .await
        .map_err(|e| CliError::connection_failed(args.address, e))?;

    let mut events = client
        .get_actor_events(&actor_id.to_string())
        .await
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to get stored actor events: {}", e),
        })?;

    // Apply the same filtering logic as the events command
    apply_filters(&mut events, &events_args)?;

    Ok(events)
}

/// Apply filters to events (reused from events command)
fn apply_filters(events: &mut Vec<ChainEvent>, args: &crate::commands::events::EventsArgs) -> CliResult<()> {
    // Filter by event type
    if let Some(event_type) = &args.event_type {
        events.retain(|e| e.event_type.contains(event_type));
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

    // Note: Time filtering would go here but requires time parsing logic
    // For now, we'll implement basic filters and add time filtering later

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::output::OutputManager;

    #[tokio::test]
    async fn test_explore_command_invalid_actor_id() {
        let args = ExploreArgs {
            actor_id: "invalid-id".to_string(),
            address: "127.0.0.1:9000".parse().unwrap(),
            live: false,
            follow: false,
            limit: 100,
            event_type: None,
            from: None,
            to: None,
            search: None,
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
}
