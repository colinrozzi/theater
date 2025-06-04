use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::debug;

use crate::client::TheaterClient;
use std::str::FromStr;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct LogsArgs {
    /// ID of the actor to get logs from
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Follow logs in real-time
    #[arg(short, long, default_value = "false")]
    pub follow: bool,

    /// Number of lines to show (0 for all)
    #[arg(short, long, default_value = "10")]
    pub lines: usize,
}

pub fn execute(args: &LogsArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Getting logs for actor: {}", args.actor_id);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id)
        .map_err(|_| anyhow!("Invalid actor ID: {}", args.actor_id))?;

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let config = crate::config::Config::default();
        let mut client = TheaterClient::new(args.address, config);

        // Connect to the server
        client.connect().await?;

        // Get the actor events (we'll filter for log events)
        let events = client.get_actor_events(&actor_id.to_string()).await?;

        // Filter for log events and extract log messages
        let log_events: Vec<_> = events.iter().filter(|e| e.event_type == "Log").collect();

        // Limit the number of logs if requested
        let logs_to_show = if args.lines > 0 && log_events.len() > args.lines {
            &log_events[log_events.len() - args.lines..]
        } else {
            &log_events[..]
        };

        // Output the logs
        if json {
            let output = serde_json::json!({
                "actor_id": actor_id.to_string(),
                "logs": logs_to_show,
                "count": logs_to_show.len()
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!(
                "{} Logs for actor: {}",
                style("ℹ").blue().bold(),
                style(actor_id.to_string()).cyan()
            );

            if logs_to_show.is_empty() {
                println!("  No logs found.");
            } else {
                for log in logs_to_show {
                    // Log event data should contain a message field
                    let ref data = log.data;
                    if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(data) {
                        if let Some(message) = json_data.get("message").and_then(|m| m.as_str()) {
                            println!("[{}] {}", log.timestamp, message);
                        }
                    }
                }
            }
        }

        // If follow mode is enabled, subscribe to actor events and print new logs
        if args.follow {
            if !json {
                println!(
                    "\n{} Following logs in real-time. Press Ctrl+C to exit.",
                    style("ℹ").blue().bold()
                );
            }

            // Subscribe to actor events
            let event_stream = client.subscribe_to_events(&actor_id.to_string()).await?;

            // TODO: Implement subscription handling for real-time logs
            // This would require a more complex setup with a channel to receive events
            // For now, we'll just pause execution to simulate following logs

            // Simulating follow mode with a 60-second wait
            // In a real implementation, this would process events as they come in
            tokio::time::sleep(Duration::from_secs(60)).await;

            // Unsubscribe when done
            event_stream.unsubscribe().await?;
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
