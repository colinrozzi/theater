use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use tracing::debug;

use theater::id::TheaterId;

use crate::{CommandContext, error::CliError, output::formatters::ActorInspection};

#[derive(Debug, Parser)]
pub struct InspectArgs {
    /// Actor ID to inspect
    #[arg(required = true)]
    pub actor_id: TheaterId,

    /// Address of the theater server
    #[arg(short, long)]
    pub address: Option<SocketAddr>,

    /// Show detailed information
    #[arg(short, long)]
    pub detailed: bool,
}

/// Execute the inspect command asynchronously (modernized)
pub async fn execute_async(args: &InspectArgs, ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Inspecting actor: {}", args.actor_id);
    
    // Get server address from args or config
    let address = ctx.server_address(args.address);
    debug!("Connecting to server at: {}", address);

    // Create client and connect
    let client = ctx.create_client();
    client.connect().await
        .map_err(|e| CliError::connection_failed(address, e))?;

    // Collect all actor information
    debug!("Getting actor status");
    let status = client.get_actor_status(&args.actor_id.to_string()).await
        .map_err(|_e| CliError::actor_not_found(&args.actor_id.to_string()))?;

    debug!("Getting actor state");
    let state_result = client.get_actor_state(&args.actor_id.to_string()).await;
    let state = match state_result {
        Ok(ref state_value) => {
            if state_value.is_null() {
                None
            } else {
                Some(state_value)
            }
        }
        _ => None,
    };

    debug!("Getting actor events");
    let events_result = client.get_actor_events(&args.actor_id.to_string()).await;
    let events = match events_result {
        Ok(events) => events,
        Err(_) => vec![],
    };

    // TODO: Implement metrics when available
    let metrics: Option<serde_json::Value> = None;

    // Create inspection result and output
    let inspection = ActorInspection {
        id: args.actor_id.clone(),
        status: format!("{:?}", status),
        state: state.cloned(),
        events: events.clone(),
        metrics,
        detailed: args.detailed,
    };

    ctx.output.output(&inspection, None)?;
    Ok(())
}

/// Legacy wrapper for backward compatibility
pub fn execute(args: &InspectArgs, verbose: bool, json: bool) -> Result<()> {
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
