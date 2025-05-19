use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tokio::time;
use tracing::{debug, info};

use crate::cli::client::ManagementResponse;
use crate::cli::client::TheaterClient;
use crate::cli::utils::event_display::{display_events, display_single_event, EventDisplayOptions};
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct SubscribeArgs {
    /// ID of the actor to subscribe to events from (use "-" to read from stdin)
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

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

pub fn execute(args: &SubscribeArgs, _verbose: bool, json: bool) -> Result<()> {
    // Read actor ID from stdin if "-" is specified
    let actor_id_str = if args.actor_id == "-" {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    } else {
        args.actor_id.clone()
    };

    debug!("Subscribing to events for actor: {}", actor_id_str);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&actor_id_str)
        .map_err(|_| anyhow!("Invalid actor ID: {}", actor_id_str))?;

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        println!(
            "{} Subscribing to events for actor: {}",
            style("ℹ").blue().bold(),
            style(actor_id.to_string()).cyan()
        );

        if let Some(event_type) = &args.event_type {
            println!(
                "{} Filtering events by type: {}",
                style("ℹ").blue().bold(),
                style(event_type).green()
            );
        }

        let mut client = TheaterClient::new(args.address);

        // Connect to the server
        client.connect().await?;

        // Set up display options
        let display_options = EventDisplayOptions {
            format: args.format.clone(),
            detailed: args.detailed,
            json,
        };

        // Initialize event counter
        let mut events_count = 0;

        // If history flag is set, get and display historical events
        if args.history {
            // Get historical events
            let mut events = client.get_actor_events(actor_id.clone()).await?;

            // Apply event type filter if specified
            if let Some(filter) = &args.event_type {
                events.retain(|e| e.event_type.contains(filter));
            }

            // Limit the number of historical events if requested
            if args.history_limit > 0 && events.len() > args.history_limit {
                // If we have more events than the limit, take the most recent ones
                let skip_count = events.len() - args.history_limit;
                events = events.into_iter().skip(skip_count).collect();
            }

            // If there are historical events, display them
            if !events.is_empty() {
                // Display events using the shared display function
                events_count = display_events(&events, Some(&actor_id), &display_options, 0)?;
            }
        }

        // Subscribe to the actor's events
        let subscription_id = client.subscribe_to_actor(actor_id.clone()).await?;

        info!(
            "Subscribed to actor events with subscription ID: {}",
            subscription_id
        );

        // If history was not requested or no events were found, we need to print the headers now
        if !args.history || events_count == 0 {
            if display_options.format == "compact" && !display_options.json {
                println!(
                    "{:<12} {:<12} {:<25} {}",
                    "HASH", "PARENT", "EVENT TYPE", "DESCRIPTION"
                );
                println!("{}", style("─".repeat(100)).dim());
            }

            if !args.history {
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
                    println!(
                        "\n{} No events received for {} seconds, exiting.",
                        style("⏱").yellow().bold(),
                        args.timeout
                    );
                    break;
                }
            }

            // Try to receive an event with a timeout to allow checking for the global timeout
            let response =
                match time::timeout(Duration::from_secs(1), client.receive_response()).await {
                    Ok(result) => match result {
                        Ok(response) => response,
                        Err(e) => {
                            // Only return error if not a timeout
                            if !e.to_string().contains("Connection closed") {
                                return Err(e);
                            }
                            continue;
                        }
                    },
                    Err(_) => continue, // Timeout, continue to check global timeout
                };

            // Process the event
            match response {
                ManagementResponse::ActorEvent { event } => {
                    // Skip if doesn't match filter
                    if let Some(filter) = &args.event_type {
                        if !event.event_type.contains(filter) {
                            continue;
                        }
                    }

                    last_event_time = std::time::Instant::now();

                    // Display the event using the shared display function
                    display_single_event(
                        &event,
                        if display_options.json {
                            "json"
                        } else {
                            &display_options.format
                        },
                        display_options.detailed,
                    )?;

                    // Check if we've hit the limit
                    if args.limit > 0 && events_count >= args.limit {
                        println!(
                            "\n{} Reached event limit ({}), exiting.",
                            style("ℹ").blue().bold(),
                            args.limit
                        );
                        break;
                    }
                }
                ManagementResponse::ActorError { error } => {
                    // Handle actor error
                    if json {
                        let output = serde_json::json!({
                            "actor_id": actor_id.to_string(),
                            "error": error,
                        });
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        println!("{} Actor error: {}", style("ERROR").bold().red(), error);
                    }
                }
                ManagementResponse::Error { error } => {
                    return Err(anyhow!("Error from server: {:?}", error));
                }
                _ => {
                    debug!("Received unexpected response: {:?}", response);
                }
            }
        }

        // Unsubscribe before exiting
        if let Err(e) = client
            .unsubscribe_from_actor(actor_id, subscription_id)
            .await
        {
            debug!("Failed to unsubscribe: {}", e);
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
