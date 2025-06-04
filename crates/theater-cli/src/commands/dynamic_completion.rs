use clap::Parser;
use std::net::SocketAddr;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::CommandContext;

#[derive(Debug, Parser)]
pub struct DynamicCompletionArgs {
    /// The command line being completed
    #[arg(required = true)]
    pub line: String,

    /// The current word being completed
    #[arg(required = true)]
    pub current: String,

    /// Address of the theater server for dynamic completions
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

/// Generate dynamic completions based on current theater state
pub async fn execute_async(args: &DynamicCompletionArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Generating dynamic completion for: '{}'", args.line);
    debug!("Current word: '{}'", args.current);

    let completions = generate_dynamic_completions(args, ctx).await?;

    // Output completions one per line for shell consumption
    for completion in completions {
        println!("{}", completion);
    }

    Ok(())
}

/// Generate completions based on context
async fn generate_dynamic_completions(
    args: &DynamicCompletionArgs,
    _ctx: &CommandContext,
) -> CliResult<Vec<String>> {
    let words: Vec<&str> = args.line.split_whitespace().collect();
    
    // Determine what we're completing based on the command structure
    match words.as_slice() {
        // theater <command>
        ["theater"] => Ok(get_command_completions(&args.current)),
        
        // theater start <manifest_or_actor_id>
        ["theater", "start"] => get_manifest_completions(&args.current).await,
        
        // theater stop <actor_id>
        ["theater", "stop"] => get_actor_id_completions(&args.current, args.address).await,
        
        // theater state <actor_id>
        ["theater", "state"] => get_actor_id_completions(&args.current, args.address).await,
        
        // theater inspect <actor_id>
        ["theater", "inspect"] => get_actor_id_completions(&args.current, args.address).await,
        
        // theater message <actor_id>
        ["theater", "message"] => get_actor_id_completions(&args.current, args.address).await,
        
        // theater events <actor_id>
        ["theater", "events"] => get_actor_id_completions(&args.current, args.address).await,
        
        // theater channel open <actor_id>
        ["theater", "channel", "open"] => get_actor_id_completions(&args.current, args.address).await,
        
        // theater create <template>
        ["theater", "create"] => Ok(get_template_completions(&args.current)),
        
        // theater completion <shell>
        ["theater", "completion"] => Ok(get_shell_completions(&args.current)),
        
        _ => Ok(vec![]),
    }
}

/// Get available command completions
fn get_command_completions(current: &str) -> Vec<String> {
    let commands = vec![
        "build", "channel", "completion", "create", "events", 
        "inspect", "list", "list-stored", "message", "start", 
        "state", "stop", "subscribe"
    ];
    
    commands
        .into_iter()
        .filter(|cmd| cmd.starts_with(current))
        .map(|s| s.to_string())
        .collect()
}

/// Get template completions
fn get_template_completions(current: &str) -> Vec<String> {
    let templates = vec!["basic", "http"];
    
    templates
        .into_iter()
        .filter(|tmpl| tmpl.starts_with(current))
        .map(|s| s.to_string())
        .collect()
}

/// Get shell completions
fn get_shell_completions(current: &str) -> Vec<String> {
    let shells = vec!["bash", "zsh", "fish", "powershell", "elvish"];
    
    shells
        .into_iter()
        .filter(|shell| shell.starts_with(current))
        .map(|s| s.to_string())
        .collect()
}

/// Get manifest file completions
async fn get_manifest_completions(current: &str) -> CliResult<Vec<String>> {
    // Look for manifest.toml files in current directory and subdirectories
    let mut completions = Vec::new();
    
    // Add manifest files
    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name == "manifest.toml" || name.ends_with(".toml") {
                    if name.starts_with(current) {
                        completions.push(name.to_string());
                    }
                }
            }
        }
    }
    
    // Also add any stored actor IDs
    if let Ok(stored_actors) = get_stored_actor_ids().await {
        for actor_id in stored_actors {
            if actor_id.starts_with(current) {
                completions.push(actor_id);
            }
        }
    }
    
    Ok(completions)
}

/// Get running actor ID completions
async fn get_actor_id_completions(current: &str, address: SocketAddr) -> CliResult<Vec<String>> {
    debug!("Getting actor completions from server at: {}", address);
    
    match get_running_actor_ids(address).await {
        Ok(actor_ids) => {
            let completions = actor_ids
                .into_iter()
                .filter(|id| id.starts_with(current))
                .collect();
            Ok(completions)
        }
        Err(e) => {
            debug!("Failed to get running actors: {}", e);
            // Fallback to stored actor IDs if server is unavailable
            get_stored_actor_ids().await.map(|ids| {
                ids.into_iter()
                    .filter(|id| id.starts_with(current))
                    .collect()
            })
        }
    }
}

/// Get running actor IDs from the server
async fn get_running_actor_ids(address: SocketAddr) -> CliResult<Vec<String>> {
    use bytes::Bytes;
    use futures::{SinkExt, StreamExt};
    use tokio::net::TcpStream;
    use tokio_util::codec::{Framed, LengthDelimitedCodec};

    let socket = TcpStream::connect(address)
        .await
        .map_err(|e| CliError::connection_failed(address, e))?;

    let mut framed = Framed::new(socket, LengthDelimitedCodec::new());

    // Send list request
    let request = serde_json::json!({
        "type": "list_actors"
    });
    
    let request_bytes = Bytes::from(serde_json::to_vec(&request).unwrap());
    framed.send(request_bytes).await.map_err(|e| {
        CliError::NetworkError {
            operation: "send list request".to_string(),
            source: Box::new(e),
        }
    })?;

    // Read response
    if let Some(response_frame) = framed.next().await {
        let response_bytes = response_frame.map_err(|e| CliError::NetworkError {
            operation: "receive list response".to_string(),
            source: Box::new(e),
        })?;

        let response: serde_json::Value = serde_json::from_slice(&response_bytes)
            .map_err(|e| CliError::InvalidResponse {
                message: "Failed to parse actor list response".to_string(),
                source: Some(Box::new(e)),
            })?;

        if let Some(actors) = response.get("actors").and_then(|a| a.as_array()) {
            let actor_ids = actors
                .iter()
                .filter_map(|actor| {
                    actor.get("id").and_then(|id| id.as_str().map(|s| s.to_string()))
                })
                .collect();
            
            return Ok(actor_ids);
        }
    }

    Ok(vec![])
}

/// Get stored actor IDs from filesystem
async fn get_stored_actor_ids() -> CliResult<Vec<String>> {
    // This would read from your stored actors directory
    // For now, return empty vec as a placeholder
    Ok(vec![])
}
