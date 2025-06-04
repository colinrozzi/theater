use std::net::SocketAddr;
use clap::Parser;
use tracing::debug;

use crate::error::CliResult;
use crate::output::formatters::ActorList;
use crate::{CommandContext, client::TheaterClient};

#[derive(Debug, Parser)]
pub struct ListArgs {
    /// Address of the theater server
    #[arg(short, long)]
    pub address: Option<SocketAddr>,
}

/// Execute the list command asynchronously
pub async fn execute_async(args: &ListArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Listing actors");
    
    let server_addr = ctx.server_address(args.address);
    debug!("Connecting to server at: {}", server_addr);

    // Create client with configuration
    let client = TheaterClient::new(server_addr, ctx.config.clone());

    // Get the list of actors
    let actors = client.list_actors().await?;

    // Create formatted output
    let actor_list = ActorList { actors };
    
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
    async fn test_list_command() {
        let args = ListArgs { address: None };
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
