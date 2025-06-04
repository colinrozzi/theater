use clap::Parser;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::output::formatters::{MessageResponse, MessageSent};
use crate::CommandContext;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct MessageArgs {
    /// ID of the actor to send a message to
    #[arg(required = true)]
    pub actor_id: String,

    /// Message to send (as string)
    #[arg(required_unless_present = "file")]
    pub message: Option<String>,

    /// File containing message to send
    #[arg(short, long, conflicts_with = "message")]
    pub file: Option<PathBuf>,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Send as a request (awaits response) instead of a one-way message
    #[arg(short, long, default_value = "false")]
    pub request: bool,
}

/// Execute the message command asynchronously with modern patterns
pub async fn execute_async(args: &MessageArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Sending message to actor: {}", args.actor_id);

    // Get message content either from direct argument or file
    let message_content = if let Some(message) = &args.message {
        message.clone()
    } else if let Some(file_path) = &args.file {
        debug!("Reading message from file: {:?}", file_path);
        fs::read_to_string(file_path).map_err(|e| CliError::FileOperationFailed {
            path: file_path.display().to_string(),
            operation: "read".to_string(),
            source: e,
        })?
    } else {
        return Err(CliError::InvalidInput {
            field: "message".to_string(),
            value: "none".to_string(),
            suggestion: "Either provide a message directly or specify a file with --file"
                .to_string(),
        });
    };

    debug!("Message: {}", message_content);
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

    // Convert message to bytes
    let message_bytes = message_content.as_bytes().to_vec();

    if args.request {
        // Send as a request and wait for response
        let response: Vec<u8> = client
            .request_message(&actor_id.to_string(), message_bytes)
            .await
            .map_err(|e| CliError::ServerError {
                message: format!("Failed to send request to actor: {}", e),
            })?;

        // Create formatted output for response
        let message_response = MessageResponse {
            actor_id: actor_id.to_string(),
            request: message_content,
            response: String::from_utf8_lossy(&response).to_string(),
        };

        // Output using the configured format
        let format = if ctx.json { Some("json") } else { None };
        ctx.output.output(&message_response, format)?;
    } else {
        // Send as a one-way message
        client
            .send_message(&actor_id.to_string(), message_bytes)
            .await
            .map_err(|e| CliError::ServerError {
                message: format!("Failed to send message to actor: {}", e),
            })?;

        // Create formatted output for message sent
        let message_sent = MessageSent {
            actor_id: actor_id.to_string(),
            message: message_content,
            success: true,
        };

        // Output using the configured format
        let format = if ctx.json { Some("json") } else { None };
        ctx.output.output(&message_sent, format)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::output::OutputManager;

    #[tokio::test]
    async fn test_message_command_invalid_actor_id() {
        let args = MessageArgs {
            actor_id: "invalid-id".to_string(),
            message: Some("test message".to_string()),
            file: None,
            address: "127.0.0.1:9000".parse().unwrap(),
            request: false,
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

    #[tokio::test]
    async fn test_message_command_no_message_or_file() {
        let args = MessageArgs {
            actor_id: "test-id".to_string(),
            message: None,
            file: None,
            address: "127.0.0.1:9000".parse().unwrap(),
            request: false,
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
            assert_eq!(field, "message");
        } else {
            panic!("Expected InvalidInput error");
        }
    }
}
