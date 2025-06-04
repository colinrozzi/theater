use clap::Parser;
use std::str::FromStr;
use theater::id::TheaterId;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::output::formatters::StoredActorList;
use crate::CommandContext;

#[derive(Debug, Parser)]
pub struct ListStoredArgs {
    /// Directory where chains are stored (defaults to THEATER_HOME/chains)
    #[arg(long)]
    pub chains_dir: Option<String>,
}

/// Execute the list-stored command asynchronously with modern patterns
pub async fn execute_async(args: &ListStoredArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Listing stored actors");

    // Determine chains directory
    let theater_home = std::env::var("THEATER_HOME")
        .unwrap_or_else(|_| format!("{}/.theater", std::env::var("HOME").unwrap_or_default()));

    let chains_dir = args
        .chains_dir
        .clone()
        .unwrap_or_else(|| format!("{}/chains", theater_home));

    debug!("Checking chains directory: {}", chains_dir);

    // Read the chains directory
    let path = std::path::Path::new(&chains_dir);

    if !path.exists() {
        // Create formatted output for empty result
        let stored_list = StoredActorList {
            actor_ids: Vec::new(),
            chains_dir: chains_dir.clone(),
            directory_exists: false,
        };

        // Output using the configured format
        let format = if ctx.json { Some("json") } else { None };
        ctx.output.output(&stored_list, format)?;
        return Ok(());
    }

    // Collect actor IDs from filenames in the chains directory
    let mut actor_ids = Vec::new();

    let read_dir = std::fs::read_dir(path).map_err(|e| CliError::FileOperationFailed {
        operation: "read directory".to_string(),
        path: chains_dir.clone(),
        source: e,
    })?;

    for entry in read_dir {
        let entry = entry.map_err(|e| CliError::FileOperationFailed {
            operation: "read directory entry".to_string(),
            path: chains_dir.clone(),
            source: e,
        })?;

        let file_path = entry.path();

        if file_path.is_file() {
            if let Some(file_name) = file_path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    // Try to parse as TheaterId
                    if let Ok(actor_id) = TheaterId::from_str(name_str) {
                        actor_ids.push(actor_id.to_string());
                    }
                }
            }
        }
    }

    // Create formatted output
    let stored_list = StoredActorList {
        actor_ids,
        chains_dir,
        directory_exists: true,
    };

    // Output using the configured format
    let format = if ctx.json { Some("json") } else { None };
    ctx.output.output(&stored_list, format)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::output::OutputManager;

    #[tokio::test]
    async fn test_list_stored_command_nonexistent_directory() {
        let args = ListStoredArgs {
            chains_dir: Some("/nonexistent/directory".to_string()),
        };
        let config = Config::default();
        let output = OutputManager::new(config.output.clone());

        let ctx = CommandContext {
            config,
            output,
            verbose: false,
            json: false,
        };

        // Should not error, just return empty list
        let result = execute_async(&args, &ctx).await;
        assert!(result.is_ok());
    }
}
