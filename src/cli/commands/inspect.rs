use anyhow::{anyhow, Result};
use clap::Parser;
use serde_json::json;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};
use tracing::debug;

use theater::id::TheaterId;
use theater::theater_server::ManagementResponse;

use crate::cli::client::TheaterClient;
use crate::cli::utils::formatting;

#[derive(Debug, Parser)]
pub struct InspectArgs {
    /// Actor ID to inspect
    #[arg(required = true)]
    pub actor_id: TheaterId,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Show detailed information
    #[arg(short, long)]
    pub detailed: bool,
}

pub fn execute(args: &InspectArgs, verbose: bool, json: bool) -> Result<()> {
    debug!("Inspecting actor: {}", args.actor_id);
    debug!("Connecting to server at: {}", args.address);

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);

        // Connect to the server
        client.connect().await?;

        // Collect all actor information
        debug!("Getting actor status");
        let status = client.get_actor_status(args.actor_id.clone()).await?;

        debug!("Getting actor state");
        let state_result = client.get_actor_state(args.actor_id.clone()).await;
        let state = match state_result {
            Ok(Some(ref state)) => {
                // Try to parse as JSON for display
                match serde_json::from_slice::<serde_json::Value>(&state) {
                    Ok(parsed) => Some(parsed),
                    Err(_) => {
                        // If it's not valid JSON, just note the size
                        debug!("State is not valid JSON, showing size only");
                        None
                    }
                }
            }
            _ => None,
        };

        debug!("Getting actor events");
        let events_result = client.get_actor_events(args.actor_id.clone()).await;
        let events = match events_result {
            Ok(events) => events,
            Err(_) => vec![],
        };

        debug!("Getting actor metrics");
        let metrics_result = client.get_actor_metrics(args.actor_id.clone()).await;
        let metrics = match metrics_result {
            Ok(metrics) => Some(metrics),
            Err(_) => None,
        };

        // Output the result
        if json {
            // JSON output format
            let output = json!({
                "id": args.actor_id.to_string(),
                "status": format!("{:?}", status),
                "state": state,
                "events": {
                    "count": events.len(),
                    "latest": events.last().map(|e| {
                        json!({
                            "type": e.event_type,
                            "timestamp": e.timestamp,
                            "hash": hex::encode(&e.hash)
                        })
                    })
                },
                "metrics": metrics
            });

            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            // Human-readable output
            println!("{}", formatting::format_section("ACTOR INFORMATION"));
            println!(
                "{}",
                formatting::format_key_value("ID", &formatting::format_id(&args.actor_id))
            );
            println!(
                "{}",
                formatting::format_key_value("Status", &formatting::format_status(&status))
            );

            // Calculate uptime if we have events
            if let Some(first_event) = events.first() {
                let now = chrono::Utc::now().timestamp() as u64;
                let uptime = Duration::from_secs(now.saturating_sub(first_event.timestamp));
                println!(
                    "{}",
                    formatting::format_key_value("Uptime", &formatting::format_duration(uptime))
                );
            }

            // State information
            println!("{}", formatting::format_section("STATE"));
            match &state {
                Some(state_json) => {
                    // Pretty print state if it's not too large
                    let state_str = serde_json::to_string_pretty(&state_json)?;
                    if state_str.len() < 1000 || args.detailed {
                        println!("{}", state_str);
                    } else {
                        println!("{} bytes of JSON data", state_str.len());
                        println!("(Use --detailed to see full state)");
                    }
                }
                None => {
                    if let Ok(Some(raw_state)) = state_result {
                        println!("{} bytes of binary data", raw_state.len());
                    } else {
                        println!("No state available");
                    }
                }
            }

            // Events information
            println!("{}", formatting::format_section("EVENTS"));
            println!("Total events: {}", events.len());

            if !events.is_empty() {
                println!("\nLatest events:");
                // Show the last 5 events (or all if there are fewer than 5)
                let start_idx = if events.len() > 5 && !args.detailed {
                    events.len() - 5
                } else {
                    0
                };

                for (i, event) in events.iter().enumerate().skip(start_idx) {
                    println!("{}. {}", i + 1, formatting::format_event_summary(event));
                }

                if events.len() > 5 && !args.detailed {
                    println!("\n(Showing only the last 5 events. Use --detailed to see all.)");
                }
            }

            // Metrics information if available
            if let Some(metrics) = metrics {
                println!("{}", formatting::format_section("METRICS"));
                // Try to pretty-print the metrics
                println!("{}", serde_json::to_string_pretty(&metrics)?);
            }

            // Additional information if detailed mode is enabled
            if args.detailed {
                // Here you would add any additional detailed information
                // such as handler configurations, runtime details, etc.
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
