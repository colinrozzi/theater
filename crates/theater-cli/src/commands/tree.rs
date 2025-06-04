use anyhow::{anyhow, Result};
use clap::Parser;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use tracing::debug;

use theater::id::TheaterId;
use theater::messages::ActorStatus;

use crate::client::TheaterClient;
use crate::utils::formatting;

#[derive(Debug, Parser)]
pub struct TreeArgs {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Maximum depth to display (0 for unlimited)
    #[arg(short, long, default_value = "0")]
    pub depth: usize,

    /// Root actor ID to start display from (default: show all actors)
    #[arg(short, long)]
    pub root: Option<TheaterId>,
}

/// Node in the actor tree
struct ActorNode {
    id: String,
    status: String,
    children: Vec<String>,
    parent: Option<String>,
    name: String, // Actor name (from manifest if available)
}

pub fn execute(args: &TreeArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Generating actor tree");
    debug!("Connecting to server at: {}", args.address);

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let mut client = TheaterClient::new(args.address, config);

        // Connect to the server
        client.connect().await?;

        // Get the list of all actors
        let actors = client.list_actors().await?;

        if actors.is_empty() {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "actors": [],
                        "tree": {}
                    }))?
                );
            } else {
                println!(
                    "{}",
                    formatting::format_info("No actors are currently running.")
                );
            }
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

            // We'd need to get actual parent/child relationships from the server
            // For now, we'll use a simplified approach assuming no relationships
            // In a real implementation, we would query this information from the server

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
        // Note: In a real implementation, we would query the actual parent-child
        // relationships from the server. This is a placeholder for demonstration.

        // Let's assume for this example that we have this information
        // In reality, you would need to extend your server/client API to get this data

        // For JSON output
        if json {
            let mut tree_json = json!({});

            // Build a JSON representation of the tree
            for (id, node) in &nodes {
                tree_json[id.to_string()] = json!({
                    "id": node.id.to_string(),
                    "status": format!("{:?}", node.status),
                    "name": node.name,
                    "children": node.children.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                    "parent": node.parent.as_ref().map(|p| p.to_string())
                });
            }

            // Output the result
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "actors": actors.iter().map(|a| a.0.to_string()).collect::<Vec<_>>(),
                    "tree": tree_json
                }))?
            );

            return Ok(());
        }

        // For human-readable output, we need to identify root nodes
        let mut root_nodes = Vec::new();

        // If a specific root is provided, use it as the only root
        if let Some(root_id) = &args.root {
            if nodes.contains_key(root_id) {
                root_nodes.push(root_id.clone());
            } else {
                return Err(anyhow!("Root actor not found: {}", root_id));
            }
        } else {
            // Otherwise, find all nodes that have no parent
            for (id, node) in &nodes {
                if node.parent.is_none() {
                    root_nodes.push(id.clone());
                }
            }
        }

        // Print the tree header
        println!("{}", formatting::format_section("ACTOR HIERARCHY"));
        println!("Total actors: {}\n", actors.len());

        // If we have no root nodes but have actors, assume all are independent
        if root_nodes.is_empty() && !actors.is_empty() {
            root_nodes = actors.into_iter().map(|a| a.0).collect();
        }

        // Print each tree starting from root nodes
        for root_id in root_nodes {
            print_tree(&nodes, &root_id, "", true, args.depth, 0);
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

/// Recursively print the tree structure
fn print_tree(
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
            formatting::format_short_id(&node.id),
            formatting::format_status(&node.status)
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
