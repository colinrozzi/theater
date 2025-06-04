use std::net::SocketAddr;
use clap::Parser;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::output::formatters::ActorList;
use crate::{CommandContext};

// Re-use the existing ListArgs structure
pub use crate::commands::list::ListArgs;

/// Execute the list command asynchronously with modern patterns
pub async fn execute_async(args: &ListArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Listing actors with modern implementation");
    debug!("Connecting to server at: {}", args.address);

    // Create a simplified client that works with the current protocol
    let actors = list_actors_simple(args.address).await?;

    // Create formatted output
    let actor_list = ActorList { 
        actors: actors.into_iter()
            .map(|(id, name)| (id.to_string(), name))
            .collect()
    };
    
    // Output using the configured format
    let format = if ctx.json { Some("json") } else { None };
    ctx.output.output(&actor_list, format)?;

    Ok(())
}

/// Simplified actor listing that works with current protocol
async fn list_actors_simple(address: SocketAddr) -> CliResult<Vec<(theater::id::TheaterId, String)>> {
    use tokio::net::TcpStream;
    use tokio_util::codec::{Framed, LengthDelimitedCodec};
    use futures::{SinkExt, StreamExt};
    use bytes::Bytes;
    
    // Connect directly to the server
    let socket = TcpStream::connect(address).await
        .map_err(|e| CliError::connection_failed(address, e))?;
    
    let mut codec = LengthDelimitedCodec::new();
    codec.set_max_frame_length(32 * 1024 * 1024);
    let mut framed = Framed::new(socket, codec);
    
    // Send ListActors command
    let command = theater_server::ManagementCommand::ListActors;
    let command_bytes = serde_json::to_vec(&command)
        .map_err(CliError::Serialization)?;
    
    framed.send(Bytes::from(command_bytes)).await
        .map_err(|_| CliError::ConnectionLost)?;
    
    // Receive response
    if let Some(response_bytes) = framed.next().await {
        let response_bytes = response_bytes
            .map_err(|_| CliError::ConnectionLost)?;
        
        let response: theater_server::ManagementResponse = serde_json::from_slice(&response_bytes)
            .map_err(|e| CliError::ProtocolError {
                reason: format!("Failed to deserialize response: {}", e),
            })?;
        
        match response {
            theater_server::ManagementResponse::ActorList { actors } => {
                debug!("Listed {} actors", actors.len());
                Ok(actors)
            }
            theater_server::ManagementResponse::Error { error } => {
                Err(CliError::ServerError { 
                    message: format!("{:?}", error)
                })
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    } else {
        Err(CliError::ConnectionLost)
    }
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
            address: "127.0.0.1:9000".parse().unwrap() 
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
