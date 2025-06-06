use clap::Parser;
use std::net::SocketAddr;
use tracing::debug;

use crate::client::cli_wrapper;
use crate::error::CliResult;
use crate::output::formatters::ActorList;
use crate::CommandContext;

#[derive(Debug, Parser)]
pub struct ListArgs {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

/// Execute the list command asynchronously using theater-client
pub async fn execute_async(args: &ListArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Listing actors using theater-client");
    debug!("Connecting to server at: {}", args.address);

    // Use the CLI wrapper - all the timeout/retry logic is handled internally
    let actors = cli_wrapper::list_actors(args.address, &ctx.config).await?;

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
    async fn test_list_command_structure() {
        let args = ListArgs {
            address: "127.0.0.1:9000".parse().unwrap(),
        };
        let config = Config::default();
        let output = OutputManager::new(config.output.clone());

        let ctx = CommandContext {
            config,
            output,
            verbose: false,
            json: false,
        };

        // This would fail without a server, but tests the structure
        let result = execute_async(&args, &ctx).await;
        assert!(result.is_err()); // Expected to fail without server
    }
}
