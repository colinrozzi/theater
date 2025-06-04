use anyhow::Result;
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tokio::time;
use tracing::{debug, info};

use crate::{CommandContext, error::CliError, output::formatters::EventSubscription};
use crate::client::ManagementResponse;
use crate::utils::event_display::{display_events, display_single_event, EventDisplayOptions};
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct SubscribeArgs {
    /// ID of the actor to subscribe to events from (use "-" to read from stdin)
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long)]
    pub address: Option<SocketAddr>,

    /// Filter events by type (e.g., http.request, filesystem.read)
    #[arg(short, long)]
    pub event_type: Option<String>,

    /// Show detailed event information
    #[arg(short, long)]
    pub detailed: bool,

    /// Maximum number of events to show (0 for unlimited)
    #[arg(short, long, default_value = "0")]
    pub limit: usize,

    /// Exit after timeout seconds with no events (0 for no timeout)
    #[arg(short, long, default_value = "0")]
    pub timeout: u64,

    /// Output format (pretty, compact, json)
    #[arg(short, long, default_value = "compact")]
    pub format: String,

    /// Show historical events before subscribing to new events
    #[arg(short = 'H', long)]
    pub history: bool,

    /// Number of historical events to show (0 for all)
    #[arg(long, default_value = "0")]
    pub history_limit: usize,
}

/// Execute the subscribe command asynchronously (modernized)
pub async fn execute_async(args: &SubscribeArgs, ctx: &CommandContext) -> Result<(), CliError> {
    // Read actor ID from stdin if "-" is specified
    let actor_id_str = if args.actor_id == "-" {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)
            .map_err(|e| CliError::invalid_input("actor_id", "-", format!("Failed to read from stdin: {}", e)))?;
        input.trim().to_string()
    } else {
        args.actor_id.clone()
    };

    debug!("Subscribing to events for actor: {}", actor_id_str);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&actor_id_str)
        .map_err(|_| CliError::invalid_actor_id(&actor_id_str))?;

    // Get server address from args or config
    let address = ctx.server_address(args.address);
    debug!("Connecting to server at: {}", address);

    // Create client and connect
    let client = ctx.create_client();
    client.connect().await
        .map_err(|e| CliError::connection_failed(address, e))?;

    // Set up display options
    let display_options = EventDisplayOptions {
        format: args.format.clone(),
        detailed: args.detailed,
        json: ctx.json,
    };

    // Initialize event counter and subscription info
    let mut events_count = 0;
    let mut subscription_info = EventSubscription {
        actor_id: actor_id.clone(),
        address: address.to_string(),
        event_type_filter: args.event_type.clone(),
        limit: args.limit,
        timeout: args.timeout,
        format: args.format.clone(),
        show_history: args.history,
        history_limit: args.history_limit,
        detailed: args.detailed,
        events_received: 0,
        subscription_id: None,
        is_active: false,
    };

    // Display subscription start info
    if !ctx.json {
        ctx.output.output(&subscription_info, None)?;
    }

    // If history flag is set, get and display historical events
    if args.history {
        let mut events = client.get_actor_events(&actor_id.to_string()).await
            .map_err(|e| CliError::actor_not_found(format!("Failed to get events for actor {}: {}", actor_id, e)))?;

        // Apply event type filter if specified
        if let Some(filter) = &args.event_type {
            events.retain(|e| e.event_type.contains(filter));
        }

        // Limit the number of historical events if requested
        if args.history_limit > 0 && events.len() > args.history_limit {
            let skip_count = events.len() - args.history_limit;
            events = events.into_iter().skip(skip_count).collect();
        }

        // Display historical events
        if !events.is_empty() {
            events_count = display_events(&events, Some(&actor_id), &display_options, 0)
                .map_err(|e| CliError::invalid_input("event_display", "events", e.to_string()))?;
        }
    }

    // Subscribe to the actor's events
    let event_stream = client.subscribe_to_events(&actor_id.to_string()).await
        .map_err(|e| CliError::actor_not_found(format!("Failed to subscribe to actor {}: {}", actor_id, e)))?;
    
    let subscription_id = event_stream.subscription_id();
    subscription_info.subscription_id = Some(subscription_id.to_string());
    subscription_info.is_active = true;

    info!("Subscribed to actor events with subscription ID: {}", subscription_id);

    // If history was not requested or no events were found, print headers
    if !args.history || events_count == 0 {
        if display_options.format == "compact" && !display_options.json {
            println!("{:<12} {:<12} {:<25} {}", "HASH", "PARENT", "EVENT TYPE", "DESCRIPTION");
            println!("{}", style("─".repeat(100)).dim());
        }

        if !args.history && !ctx.json {
            println!("{} Waiting for events...\n", style("⏳").yellow().bold());
        }
    }

    let mut last_event_time = std::time::Instant::now();

    // Listen for events
    loop {
        // Check for timeout if enabled
        if args.timeout > 0 {
            let timeout_duration = Duration::from_secs(args.timeout);
            if last_event_time.elapsed() > timeout_duration {
                if !ctx.json {
                    println!("\n{} No events received for {} seconds, exiting.",
                        style("⏱").yellow().bold(), args.timeout);
                }
                break;
            }
        }

        // Try to receive an event with a timeout
        let response = match time::timeout(Duration::from_secs(1), client.next_response()).await {
            Ok(result) => match result {
                Ok(response) => response,
                Err(e) => {
                    if !e.to_string().contains("Connection closed") {
                        return Err(CliError::connection_failed(address, e));
                    }
                    continue;
                }
            },
            Err(_) => continue, // Timeout, continue to check global timeout
        };

        // Process the event
        match response {
            Some(ManagementResponse::ActorEvent { event }) => {
                // Skip if doesn't match filter
                if let Some(filter) = &args.event_type {
                    if !event.event_type.contains(filter) {
                        continue;
                    }
                }

                last_event_time = std::time::Instant::now();
                events_count += 1;
                subscription_info.events_received = events_count;

                // Display the event
                display_single_event(
                    &event,
                    if display_options.json { "json" } else { &display_options.format },
                    display_options.detailed,
                ).map_err(|e| CliError::invalid_input("event_display", "event", e.to_string()))?;

                // Check if we've hit the limit
                if args.limit > 0 && events_count >= args.limit {
                    if !ctx.json {
                        println!("\n{} Reached event limit ({}), exiting.",
                            style("ℹ").blue().bold(), args.limit);
                    }
                    break;
                }
            }
            Some(ManagementResponse::ActorError { error }) => {
                if ctx.json {
                    let output = serde_json::json!({
                        "actor_id": actor_id.to_string(),
                        "error": error,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)
                        .map_err(|e| CliError::invalid_input("json_output", "error", e.to_string()))?);
                } else {
                    println!("{} Actor error: {}", style("ERROR").bold().red(), error);
                }
            }
            Some(ManagementResponse::Error { error }) => {
                return Err(CliError::actor_not_found(format!("Server error: {:?}", error)));
            }
            _ => {
                debug!("Received unexpected response: {:?}", response);
            }
        }
    }

    // Unsubscribe before exiting
    if let Err(e) = client.unsubscribe_from_actor(&actor_id.to_string(), subscription_id).await {
        debug!("Failed to unsubscribe: {}", e);
    }

    subscription_info.is_active = false;
    
    // Final status output if not JSON
    if !ctx.json && !args.history {
        subscription_info.events_received = events_count;
        // Could output final status here if desired
    }

    Ok(())
}

/// Legacy wrapper for backward compatibility
pub fn execute(args: &SubscribeArgs, verbose: bool, json: bool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let output = crate::output::OutputManager::new(config.output.clone());
        let ctx = crate::CommandContext {
            config,
            output,
            verbose,
            json,
        };
        execute_async(args, &ctx).await.map_err(|e| anyhow::Error::from(e))
    })
}
