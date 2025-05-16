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

        // Subscribe to the actor's events
        let subscription_id = client.subscribe_to_actor(actor_id.clone()).await?;

        info!(
            "Subscribed to actor events with subscription ID: {}",
            subscription_id
        );
        println!("{} Waiting for events...\n", style("⏳").yellow().bold());

        let mut events_count = 0;
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
                ManagementResponse::ActorEvent { id, event } => {
                    // Skip if doesn't match filter
                    if let Some(filter) = &args.event_type {
                        if !event.event_type.contains(filter) {
                            continue;
                        }
                    }

                    last_event_time = std::time::Instant::now();
                    events_count += 1;

                    // Format and display the event
                    if json {
                        let output = serde_json::json!({
                            "actor_id": id.to_string(),
                            "event": event,
                        });
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
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
                        if args.detailed {
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
                ManagementResponse::ActorError { id, error } => {
                    // Handle actor error
                    if json {
                        let output = serde_json::json!({
                            "actor_id": id.to_string(),
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
