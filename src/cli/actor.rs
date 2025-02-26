use crate::id::TheaterId;
use crate::theater_server::{ManagementCommand, ManagementResponse};
use anyhow::Result;
use bytes::Bytes;
use clap::{Args, Subcommand};
use console::style;
use dialoguer::{theme::ColorfulTheme, Select};
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

#[derive(Args)]
pub struct ActorArgs {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: String,

    #[command(subcommand)]
    pub command: ActorCommands,
}

#[derive(Subcommand)]
pub enum ActorCommands {
    /// Start a new actor
    Start {
        /// Path to the actor manifest
        #[arg(value_name = "MANIFEST")]
        manifest: Option<PathBuf>,
    },
    /// Stop an actor
    Stop {
        /// Actor ID to stop
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,
    },
    /// List all running actors
    List {
        /// Show detailed information about each actor
        #[arg(short, long)]
        detailed: bool,
    },
    /// Subscribe to actor events
    Subscribe {
        /// Actor ID to subscribe to
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,
    },
    /// Inspect a running actor
    Inspect {
        /// Actor ID to inspect
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,
    },
    /// Restart an actor
    Restart {
        /// Actor ID to restart
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,
    },
    /// View actor logs
    Logs {
        /// Actor ID to view logs for
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,

        /// Number of lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },
    /// View or export actor state
    State {
        /// Actor ID to view state for
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,

        /// Export state to file
        #[arg(short, long)]
        export: Option<PathBuf>,

        /// Format (json, yaml, toml)
        #[arg(short, long, default_value = "json")]
        format: String,
    },
    /// Send a message to an actor
    Call {
        /// Actor ID to call
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,

        /// Message type/method
        #[arg(short, long)]
        method: String,

        /// Message payload as JSON
        #[arg(short, long)]
        payload: Option<String>,

        /// Read payload from file
        #[arg(short = 'f', long)]
        payload_file: Option<PathBuf>,
    },
}

pub async fn handle_actor_command(args: ActorArgs) -> Result<()> {
    match &args.command {
        ActorCommands::Start { manifest } => start_actor(manifest.clone(), &args.address).await,
        ActorCommands::Stop { id } => stop_actor(id.clone(), &args.address).await,
        ActorCommands::List { detailed } => list_actors(*detailed, &args.address).await,
        ActorCommands::Subscribe { id } => subscribe_to_actor(id.clone(), &args.address).await,
        ActorCommands::Inspect { id } => inspect_actor(id.clone(), &args.address).await,
        ActorCommands::Restart { id } => restart_actor(id.clone(), &args.address).await,
        ActorCommands::Logs { id, lines, follow } => {
            view_actor_logs(id.clone(), *lines, *follow, &args.address).await
        }
        ActorCommands::State { id, export, format } => {
            view_actor_state(id.clone(), export.clone(), format, &args.address).await
        }
        ActorCommands::Call {
            id,
            method,
            payload,
            payload_file,
        } => {
            call_actor(
                id.clone(),
                method,
                payload.clone(),
                payload_file.clone(),
                &args.address,
            )
            .await
        }
    }
}

/// Connect to the theater server
async fn connect_to_server(address: &str) -> Result<Framed<TcpStream, LengthDelimitedCodec>> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message(format!("Connecting to theater server at {}", address));
    spinner.enable_steady_tick(Duration::from_millis(80));

    let stream = match TcpStream::connect(address).await {
        Ok(stream) => {
            spinner.finish_with_message(format!("Connected to theater server at {}", address));
            stream
        }
        Err(e) => {
            spinner.finish_with_message(format!("{}", style("Connection failed").red()));
            return Err(anyhow::anyhow!("Failed to connect: {}", e));
        }
    };

    Ok(Framed::new(stream, LengthDelimitedCodec::new()))
}

/// Get a list of running actors
async fn get_actor_list(address: &str) -> Result<Vec<TheaterId>> {
    let mut framed = connect_to_server(address).await?;

    // Send list command
    let command = ManagementCommand::ListActors;
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    // Get response
    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;
        match response {
            ManagementResponse::ActorList { actors } => {
                return Ok(actors);
            }
            _ => {
                return Err(anyhow::anyhow!("Unexpected response from server"));
            }
        }
    }

    Err(anyhow::anyhow!("Failed to get actor list"))
}

/// Let user select an actor if ID is not provided
async fn select_actor(id_opt: Option<String>, address: &str) -> Result<TheaterId> {
    match id_opt {
        Some(id_str) => Ok(id_str.parse()?),
        None => {
            let actors = get_actor_list(address).await?;

            if actors.is_empty() {
                return Err(anyhow::anyhow!("No running actors found"));
            }

            let actor_strings: Vec<String> = actors.iter().map(|id| id.to_string()).collect();

            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select an actor")
                .default(0)
                .items(&actor_strings)
                .interact()?;

            Ok(actors[selection].clone()) // Fixed: Added .clone()
        }
    }
}

// Implementation of basic actor commands reusing existing functionality
async fn start_actor(manifest: Option<PathBuf>, address: &str) -> Result<()> {
    super::legacy::execute_command(super::legacy::Commands::Start { manifest }, address).await
}

async fn stop_actor(id: Option<String>, address: &str) -> Result<()> {
    super::legacy::execute_command(super::legacy::Commands::Stop { id }, address).await
}

async fn list_actors(detailed: bool, address: &str) -> Result<()> {
    super::legacy::execute_command(super::legacy::Commands::List { detailed }, address).await
}

async fn subscribe_to_actor(id: Option<String>, address: &str) -> Result<()> {
    super::legacy::execute_command(super::legacy::Commands::Subscribe { id }, address).await
}

// New advanced actor commands
async fn inspect_actor(id_opt: Option<String>, address: &str) -> Result<()> {
    println!("{}", style("Theater Actor Inspector").bold().cyan());

    let actor_id = select_actor(id_opt, address).await?;

    // For now, until server support is added, we'll just display the existing
    // actor information we can get from list command
    let mut framed = connect_to_server(address).await?;

    // We'll use ListActors to get basic information
    let command = ManagementCommand::ListActors;
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    println!(
        "{} Fetching details for actor {}",
        style("INFO:").blue().bold(),
        style(actor_id.clone()).green() // Fixed: Added .clone()
    );

    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorList { actors } => {
                if !actors.contains(&actor_id) {
                    println!(
                        "{} Actor not found: {}",
                        style("Error:").red().bold(),
                        style(&actor_id).red()
                    );
                    return Ok(());
                }

                // Until we have the ActorDetails command, display a placeholder
                println!("\n{}", style("Actor Details").bold().underlined());
                println!("ID:           {}", style(&actor_id).green());
                println!("Status:       {}", style("Running").yellow());
                println!("\nNOTE: For more detailed information, server support for actor inspection needs to be implemented.");
            }
            ManagementResponse::Error { message } => {
                println!("{} {}", style("Error:").red().bold(), message);
            }
            _ => {
                println!(
                    "{} Unexpected response from server",
                    style("Error:").red().bold()
                );
            }
        }
    } else {
        println!(
            "{} No response received from server",
            style("Error:").red().bold()
        );
    }

    Ok(())
}

async fn restart_actor(id_opt: Option<String>, address: &str) -> Result<()> {
    println!("{}", style("Theater Actor Restart").bold().cyan());

    let actor_id = select_actor(id_opt, address).await?;

    // For now, without extended server support, we can emulate restart by stopping and starting
    let mut framed = connect_to_server(address).await?;

    // First, get the manifest information by listing the actors
    // Note: This is a placeholder. In the real implementation, we would get manifest from the server
    println!(
        "{} To restart actor {}, we need the manifest (not yet implemented)",
        style("Note:").yellow().bold(),
        style(actor_id.clone()).green() // Fixed: Added .clone()
    );

    println!(
        "{} For now, we can only stop the actor",
        style("Note:").yellow().bold()
    );

    // Stop actor
    let command = ManagementCommand::StopActor {
        id: actor_id.clone(),
    }; // Fixed: Added .clone()
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorStopped { id } => {
                println!(
                    "{} Actor {} stopped successfully",
                    style("✓").green().bold(),
                    style(id).yellow()
                );
            }
            ManagementResponse::Error { message } => {
                println!("{} {}", style("Error:").red().bold(), message);
                return Ok(());
            }
            _ => {
                println!(
                    "{} Unexpected response from server",
                    style("Error:").red().bold()
                );
                return Ok(());
            }
        }
    }

    println!(
        "\n{} When server support is added, the actor will also be restarted automatically.",
        style("Note:").yellow().bold()
    );

    Ok(())
}

async fn view_actor_logs(
    id_opt: Option<String>,
    lines: usize,
    follow: bool,
    address: &str,
) -> Result<()> {
    println!("{}", style("Theater Actor Logs").bold().cyan());

    let actor_id = select_actor(id_opt, address).await?;

    println!(
        "{} This feature requires server-side support for actor logs",
        style("Note:").yellow().bold()
    );

    println!(
        "Would fetch last {} lines of logs for actor {}",
        lines,
        style(actor_id).green()
    );

    if follow {
        println!("Would also follow log updates in real time");
    }

    // Placeholder for future implementation
    println!("\n{}", style("Sample Log Format:").bold().underlined());
    println!(
        "[{}] {} Actor initialized",
        style("2023-02-26 12:34:56").dim(),
        style("INFO").blue().bold()
    );
    println!(
        "[{}] {} State updated: version=2",
        style("2023-02-26 12:35:01").dim(),
        style("INFO").blue().bold()
    );
    println!(
        "[{}] {} Received message type='test'",
        style("2023-02-26 12:35:10").dim(),
        style("DEBUG").dim()
    );

    Ok(())
}

async fn view_actor_state(
    id_opt: Option<String>,
    export: Option<PathBuf>,
    format: &str,
    address: &str,
) -> Result<()> {
    println!("{}", style("Theater Actor State").bold().cyan());

    let actor_id = select_actor(id_opt, address).await?;

    println!(
        "{} This feature requires server-side support for accessing actor state",
        style("Note:").yellow().bold()
    );

    println!(
        "Would fetch state for actor {} in {} format",
        style(actor_id).green(),
        style(format).yellow()
    );

    if let Some(path) = export {
        println!("Would export state to: {}", style(path.display()).green());
    }

    // Placeholder for future implementation
    println!("\n{}", style("Sample State Format:").bold().underlined());

    let example_state = r#"{
  "counter": 42,
  "lastUpdated": "2023-02-26T12:34:56Z",
  "status": "running",
  "data": {
    "key1": "value1",
    "key2": "value2"
  }
}"#;

    println!("{}", example_state);

    Ok(())
}

async fn call_actor(
    id_opt: Option<String>,
    method: &str,
    payload: Option<String>,
    payload_file: Option<PathBuf>,
    address: &str,
) -> Result<()> {
    println!("{}", style("Theater Actor Call").bold().cyan());

    let actor_id = select_actor(id_opt, address).await?;

    // Determine payload from options
    let payload_data = match (payload, payload_file) {
        (Some(data), _) => data,
        (None, Some(path)) => {
            if !path.exists() {
                return Err(anyhow::anyhow!(
                    "Payload file not found: {}",
                    path.display()
                ));
            }

            std::fs::read_to_string(path)?
        }
        (None, None) => {
            // Simple empty JSON object
            "{}".to_string()
        }
    };

    // Validate that payload is valid JSON
    match serde_json::from_str::<serde_json::Value>(&payload_data) {
        Ok(_) => {
            // JSON is valid
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Invalid JSON payload: {}", e));
        }
    }

    println!(
        "{} This feature requires server-side support for calling actor methods",
        style("Note:").yellow().bold()
    );

    println!(
        "Would call method {} on actor {} with payload:",
        style(method).yellow(),
        style(actor_id).green()
    );

    // Try to pretty-print the payload
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload_data) {
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("{}", payload_data);
    }

    Ok(())
}
