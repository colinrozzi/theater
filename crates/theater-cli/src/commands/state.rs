use clap::Parser;
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::output::formatters::ActorState;
use crate::CommandContext;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct StateArgs {
    /// ID of the actor to get state from
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Output format (raw, json, pretty)
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
}

/// Execute the state command asynchronously with modern patterns
pub async fn execute_async(args: &StateArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Getting state for actor: {}", args.actor_id);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id).map_err(|_| CliError::InvalidInput {
        field: "actor_id".to_string(),
        value: args.actor_id.clone(),
        suggestion: "Provide a valid actor ID in the correct format".to_string(),
    })?;

    // Create client and connect
    let client = ctx.create_client();
    client
        .connect()
        .await
        .map_err(|e| CliError::connection_failed(args.address, e))?;

    // Get the actor state
    let state = client
        .get_actor_state(&actor_id.to_string())
        .await
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to get actor state: {}", e),
        })?;

    // Create formatted output
    let actor_state = ActorState {
        actor_id: actor_id.to_string(),
        state,
    };

    // Output using the configured format
    let format = if ctx.json {
        Some("json")
    } else {
        Some(args.format.as_str())
    };
    ctx.output.output(&actor_state, format)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::output::OutputManager;

    #[tokio::test]
    async fn test_state_command_invalid_actor_id() {
        let args = StateArgs {
            actor_id: "invalid-id".to_string(),
            address: "127.0.0.1:9000".parse().unwrap(),
            format: "pretty".to_string(),
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
