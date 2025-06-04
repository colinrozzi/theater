use clap::Parser;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::debug;
use std::str::FromStr;

use theater::id::TheaterId;
use crate::error::{CliError, CliResult};
use crate::output::formatters::ActorLogs;
use crate::CommandContext;

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

/// Execute the logs command asynchronously with modern patterns
pub async fn execute_async(args: &LogsArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Getting logs for actor: {}", args.actor_id);
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

    // Get the actor events (we'll filter for log events)
    let events = client.get_actor_events(&actor_id.to_string()).await
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to get actor events: {}", e),
        })?;

    // Filter for log events and extract log messages
    let log_events: Vec<_> = events.iter().filter(|e| e.event_type == "Log").cloned().collect();

    // Limit the number of logs if requested
    let logs_to_show = if args.lines > 0 && log_events.len() > args.lines {
        log_events[log_events.len() - args.lines..].to_vec()
    } else {
        log_events
    };

    // Create formatted output
    let actor_logs = ActorLogs {
        actor_id: actor_id.to_string(),
        events: logs_to_show,
        follow_mode: args.follow,
        lines_limit: args.lines,
    };

    // Output the logs
    let format = if ctx.json { Some("json") } else { None };
    ctx.output.output(&actor_logs, format)?;

    // If follow mode is enabled, subscribe to actor events and print new logs
    if args.follow {
        follow_logs(&client, &actor_id, ctx).await?;
    }

    Ok(())
}

/// Handle real-time log following
async fn follow_logs(
    client: &crate::client::TheaterClient,
    actor_id: &TheaterId,
    ctx: &CommandContext,
) -> CliResult<()> {
    if !ctx.json {
        ctx.output.info("Following logs in real-time. Press Ctrl+C to exit.")?;
    }

    // Subscribe to actor events
    let event_stream = client.subscribe_to_events(&actor_id.to_string()).await
        .map_err(|e| CliError::EventStreamError {
            reason: format!("Failed to subscribe to events: {}", e),
        })?;

    // TODO: Implement proper event stream handling for real-time logs
    // This would require a more complex setup with a channel to receive events
    // For now, we'll just pause execution to simulate following logs

    // Simulating follow mode with a 60-second wait
    // In a real implementation, this would process events as they come in
    tokio::time::sleep(Duration::from_secs(60)).await;

    // Unsubscribe when done
    event_stream.unsubscribe().await
        .map_err(|e| CliError::EventStreamError {
            reason: format!("Failed to unsubscribe from events: {}", e),
        })?;

    Ok(())
}

// Keep the legacy function for backward compatibility
pub fn execute(args: &LogsArgs, verbose: bool, json: bool) -> anyhow::Result<()> {
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
    async fn test_logs_command_invalid_actor_id() {
        let args = LogsArgs {
            actor_id: "invalid-id".to_string(),
            address: "127.0.0.1:9000".parse().unwrap(),
            follow: false,
            lines: 10,
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
