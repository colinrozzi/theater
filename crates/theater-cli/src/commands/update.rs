use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::debug;

use crate::client::TheaterClient;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct UpdateArgs {
    /// ID of the actor to update
    #[arg(required = true)]
    pub actor_id: String,

    /// Path or address to the new component
    #[arg(required = true)]
    pub component: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

pub async fn update_actor_component(
    client: &mut TheaterClient,
    actor_id: TheaterId,
    component: String,
) -> Result<()> {
    debug!(
        "Updating actor component for: {}, to component: {}",
        actor_id, component
    );

    // Use the built-in client method
    client.update_actor_component(&actor_id.to_string(), component).await.map_err(Into::into)
}

pub fn execute(args: &UpdateArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Updating actor component: {}", args.actor_id);
    debug!("New component: {}", args.component);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id)
        .map_err(|_| anyhow!("Invalid actor ID: {}", args.actor_id))?;

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let mut client = TheaterClient::new(args.address, config);

        // Connect to the server
        client.connect().await?;

        // Update the actor component
        update_actor_component(&mut client, actor_id.clone(), args.component.clone()).await?;

        // Output the result
        if !json {
            println!(
                "{} Updated actor: {} with component: {}",
                style("✓").green().bold(),
                style(actor_id.to_string()).cyan(),
                style(args.component.clone()).cyan()
            );
        } else {
            let output = serde_json::json!({
                "success": true,
                "actor_id": actor_id.to_string(),
                "component": args.component.clone()
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
