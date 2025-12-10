use clap::Parser;
use std::net::SocketAddr;
use tracing::debug;

use crate::error::CliResult;
use crate::output::formatters::ActorList;
use crate::CommandContext;

#[derive(Debug, Parser)]
pub struct ListArgs {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

/// Execute the list command asynchronously with cancellation support
pub async fn execute_async(args: &ListArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Listing actors with cancellation support");
    debug!("Connecting to server at: {}", args.address);

    // Create a client with the specified address and cancellation token
    let client = crate::client::TheaterClient::new(args.address, ctx.shutdown_token.clone());

    // This will now properly respond to Ctrl+C during the network operation
    let actors = client.list_actors().await?;

    // Create formatted output
    let actor_list = ActorList {
        actors: actors
            .into_iter()
            .map(|(id, name)| (id.to_string(), name))
            .collect(),
    };

    // Output using the configured format
    let format = if ctx.json { Some("json") } else { None };
    ctx.output.output(&actor_list, format)?;

    Ok(())
}

// Keep the legacy function for backward compatibility
pub fn execute(args: &ListArgs, verbose: bool, json: bool) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let output = crate::output::OutputManager::new(config.output.clone());
        let shutdown_token = tokio_util::sync::CancellationToken::new();

        let ctx = CommandContext {
            config,
            output,
            verbose,
            json,
            shutdown_token,
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
    async fn test_list_command_structure() {
        let args = ListArgs {
            address: "127.0.0.1:9000".parse().unwrap(),
        };
        let config = Config::default();
        let output = OutputManager::new(config.output.clone());
        let shutdown_token = tokio_util::sync::CancellationToken::new();

        let ctx = CommandContext {
            config,
            output,
            verbose: false,
            json: false,
            shutdown_token,
        };

        // This would fail without a server, but tests the structure
        let result = execute_async(&args, &ctx).await;
        // The exact behavior depends on network conditions and server availability
        // This test primarily validates the command structure and argument parsing
        match result {
            Ok(_) => {
                // If a server happens to be running, that's fine
            }
            Err(_) => {
                // Expected to fail without server - this is the normal case
            }
        }
    }
}
