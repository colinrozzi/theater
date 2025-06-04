use clap::Parser;
use std::net::SocketAddr;
use tracing::debug;
use std::str::FromStr;

use theater::id::TheaterId;
use crate::error::{CliError, CliResult};
use crate::output::formatters::ActorAction;
use crate::CommandContext;

#[derive(Debug, Parser)]
pub struct StopArgs {
    /// ID of the actor to stop
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

/// Execute the stop command asynchronously with modern patterns
pub async fn execute_async(args: &StopArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Stopping actor: {}", args.actor_id);
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

    // Stop the actor
    client.stop_actor(&actor_id.to_string()).await
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to stop actor: {}", e),
        })?;

    // Create formatted output
    let action_result = ActorAction {
        action: "stopped".to_string(),
        actor_id: actor_id.to_string(),
        success: true,
        message: None,
    };
    
    // Output using the configured format
    let format = if ctx.json { Some("json") } else { None };
    ctx.output.output(&action_result, format)?;

    Ok(())
}

// Keep the legacy function for backward compatibility
pub fn execute(args: &StopArgs, verbose: bool, json: bool) -> anyhow::Result<()> {
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
    async fn test_stop_command_invalid_actor_id() {
        let args = StopArgs {
            actor_id: "invalid-id".to_string(),
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
        
        let result = execute_async(&args, &ctx).await;
        assert!(result.is_err());
        if let Err(CliError::InvalidInput { field, .. }) = result {
            assert_eq!(field, "actor_id");
        } else {
            panic!("Expected InvalidInput error");
        }
    }
}
