use anyhow::Result;
use clap::Parser;
use std::collections::HashMap;
use std::net::SocketAddr;
use tracing::debug;

use theater::id::TheaterId;

use crate::{CommandContext, error::CliError, output::formatters::ActorTree};
use console::style;

#[derive(Debug, Parser)]
pub struct TreeArgs {
    /// Address of the theater server
    #[arg(short, long)]
    pub address: Option<SocketAddr>,

    /// Maximum depth to display (0 for unlimited)
    #[arg(short, long, default_value = "0")]
    pub depth: usize,

    /// Root actor ID to start display from (default: show all actors)
    #[arg(short, long)]
    pub root: Option<TheaterId>,
}

/// Node in the actor tree
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActorNode {
    pub id: String,
    pub status: String,
    pub children: Vec<String>,
    pub parent: Option<String>,
    pub name: String,
}

/// Execute the tree command asynchronously (modernized)
pub async fn execute_async(args: &TreeArgs, ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Generating actor tree");
    
    // Get server address from args or config
    let address = ctx.server_address(args.address);
    debug!("Connecting to server at: {}", address);

    // Create client and connect
    let client = ctx.create_client();
    client.connect().await
        .map_err(|e| CliError::connection_failed(address, e))?;

    // Get the list of all actors
    let actors = client.list_actors().await
        .map_err(|e| CliError::connection_failed(address, e))?;

    if actors.is_empty() {
        let tree = ActorTree {
            actors: Vec::new(),
            nodes: HashMap::new(),
            root_nodes: Vec::new(),
            max_depth: args.depth,
            specified_root: args.root.clone(),
        };
        ctx.output.output(&tree, None)?;
        return Ok(());
    }

    // Collect information about each actor
    let mut nodes: HashMap<String, ActorNode> = HashMap::new();

    // First pass: collect basic information
    for (actor_id, _name) in &actors {
        debug!("Getting information for actor: {}", actor_id);

        // Get actor status
        let status_str = match client.get_actor_status(actor_id).await {
            Ok(status) => status,
            Err(_) => "Stopped".to_string(), // Use Stopped as fallback
        };

        // In a real implementation, we would query parent/child relationships from the server
        // For now, we'll use a simplified approach assuming no relationships
        nodes.insert(
            actor_id.clone(),
            ActorNode {
                id: actor_id.clone(),
                status: status_str,
                children: Vec::new(),
                parent: None,
                name: format!("Actor {}", actor_id),
            },
        );
    }

    // Build the tree structure
    let mut root_nodes: Vec<String> = Vec::new();

    // If a specific root is provided, use it as the only root
    if let Some(root_id) = &args.root {
        let root_str = root_id.to_string();
        if nodes.contains_key(&root_str) {
            root_nodes.push(root_str);
        } else {
            return Err(CliError::actor_not_found(root_id.to_string()));
        }
    } else {
        // Find all nodes that have no parent (for now, all actors are roots)
        for (id, node) in &nodes {
            if node.parent.is_none() {
                root_nodes.push(id.clone());
            }
        }
    }

    // If we have no root nodes but have actors, assume all are independent
    if root_nodes.is_empty() && !actors.is_empty() {
        root_nodes = actors.iter().map(|a| a.0.clone()).collect();
    }

    // Create tree structure and output
    let tree = ActorTree {
        actors: actors.into_iter().map(|a| a.0).collect(),
        nodes,
        root_nodes,
        max_depth: args.depth,
        specified_root: args.root.clone(),
    };

    ctx.output.output(&tree, None)?;
    Ok(())
}

/// Legacy wrapper for backward compatibility
pub fn execute(args: &TreeArgs, verbose: bool, json: bool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let output = crate::output::OutputManager::new(config.output.clone());
        let ctx = crate::CommandContext {
            config,
            output,
            verbose,
            json,
        };
        execute_async(args, &ctx).await.map_err(|e| anyhow::Error::from(e))
    })
}

/// Format a short version of an actor ID string (first 8 chars)
pub fn format_short_id_string(id: &str) -> String {
    let short_id = &id[..std::cmp::min(8, id.len())];
    style(short_id).cyan().to_string()
}

/// Format an actor status string with appropriate color
pub fn format_status_string(status: &str) -> String {
    match status.to_uppercase().as_str() {
        "RUNNING" => style("RUNNING").green().bold().to_string(),
        "STOPPED" => style("STOPPED").red().bold().to_string(),
        "FAILED" => style("FAILED").red().bold().to_string(),
        _ => style(status).yellow().to_string(),
    }
}

/// Recursively print the tree structure
pub fn print_tree(
    nodes: &HashMap<String, ActorNode>,
    current_id: &String,
    prefix: &str,
    is_last: bool,
    max_depth: usize,
    current_depth: usize,
) {
    // Check if we've reached maximum depth (0 means unlimited)
    if max_depth > 0 && current_depth >= max_depth {
        return;
    }

    // Get the current node
    if let Some(node) = nodes.get(current_id) {
        // Determine the branch character
        let branch = if is_last { "└── " } else { "├── " };

        // Print the current node with proper indentation
        println!(
            "{}{}{} ({})",
            prefix,
            branch,
            format_short_id_string(&node.id),
            format_status_string(&node.status)
        );

        // Determine the prefix for children
        let child_prefix = if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        // Print all children
        let children = &node.children;
        for (i, child_id) in children.iter().enumerate() {
            let is_last_child = i == children.len() - 1;
            print_tree(
                nodes,
                child_id,
                &child_prefix,
                is_last_child,
                max_depth,
                current_depth + 1,
            );
        }
    }
}
