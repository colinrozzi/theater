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
use tokio::fs;
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
    /// Start a new actor from a manifest string
    StartFromString {
        /// TOML content for the manifest
        #[arg(value_name = "CONTENT")]
        content: Option<String>,
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
        ActorCommands::StartFromString { content } => {
            start_actor_from_string(content.clone(), &args.address).await
        }
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
    // Try to resolve manifest path or actor reference
    let resolved_manifest = match manifest {
        Some(path) => {
            if path.is_relative() {
                // First check if this is an actor name reference
                let path_str = path.to_string_lossy().to_string();
                if !path_str.contains("/")
                    && !path_str.contains("\\")
                    && !(path_str.ends_with(".toml") || path_str.ends_with(".wasm"))
                {
                    // This might be an actor name reference (e.g., "chat" or "chat:0.1.0")
                    let registry_path = crate::registry::get_registry_path();

                    if let Some(reg_path) = &registry_path {
                        println!(
                            "{} Trying to resolve actor reference '{}' using registry at {}",
                            style("INFO:").blue().bold(),
                            style(&path_str).yellow(),
                            style(reg_path.display()).dim()
                        );

                        match crate::registry::resolver::resolve_actor_reference(
                            &path_str,
                            registry_path.as_deref(),
                        ) {
                            Ok((resolved_path, _component_path)) => {
                                println!(
                                    "{} Resolved actor reference '{}' to {}",
                                    style("INFO:").blue().bold(),
                                    style(&path_str).yellow(),
                                    style(resolved_path.display()).green()
                                );
                                resolved_path
                            }
                            Err(e) => {
                                println!(
                                    "{} Failed to resolve actor reference '{}': {}",
                                    style("WARN:").yellow().bold(),
                                    style(&path_str).yellow(),
                                    e
                                );

                                // Fallback to treating as a relative path
                                match std::env::current_dir() {
                                    Ok(current_dir) => {
                                        let abs_path = current_dir.join(&path);
                                        println!(
                                            "{} Treating as relative path, resolving {} to {}",
                                            style("INFO:").blue().bold(),
                                            style(path.display()).dim(),
                                            style(abs_path.display()).green()
                                        );
                                        abs_path
                                    }
                                    Err(e) => {
                                        return Err(anyhow::anyhow!(
                                            "Failed to get current directory: {}",
                                            e
                                        ))
                                    }
                                }
                            }
                        }
                    } else {
                        println!(
                            "{} No registry found, treating '{}' as a relative path",
                            style("INFO:").blue().bold(),
                            style(&path_str).yellow()
                        );

                        // Resolve relative path as normal
                        match std::env::current_dir() {
                            Ok(current_dir) => {
                                let abs_path = current_dir.join(&path);
                                println!(
                                    "{} Resolving relative path {} to {}",
                                    style("INFO:").blue().bold(),
                                    style(path.display()).dim(),
                                    style(abs_path.display()).green()
                                );
                                abs_path
                            }
                            Err(e) => {
                                return Err(anyhow::anyhow!(
                                    "Failed to get current directory: {}",
                                    e
                                ))
                            }
                        }
                    }
                } else {
                    // Regular relative path
                    match std::env::current_dir() {
                        Ok(current_dir) => {
                            let abs_path = current_dir.join(&path);
                            println!(
                                "{} Resolving relative path {} to {}",
                                style("INFO:").blue().bold(),
                                style(path.display()).dim(),
                                style(abs_path.display()).green()
                            );
                            abs_path
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("Failed to get current directory: {}", e))
                        }
                    }
                }
            } else {
                // Already absolute
                path
            }
        }
        None => {
            // If no manifest provided, check if there's a default actor in current directory
            match std::env::current_dir() {
                Ok(dir) => {
                    let default_manifest = dir.join("actor.toml");
                    if default_manifest.exists() {
                        println!(
                            "{} No manifest specified, using default {}",
                            style("INFO:").blue().bold(),
                            style(default_manifest.display()).green()
                        );
                        default_manifest
                    } else {
                        return Err(anyhow::anyhow!("No manifest file provided and no default 'actor.toml' found in current directory"));
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Failed to get current directory: {}", e)),
            }
        }
    };

    // Pass the resolved path to the legacy command
    super::legacy::execute_command(
        super::legacy::Commands::Start {
            manifest: Some(resolved_manifest),
        },
        address,
    )
    .await
}

// Implementation of starting an actor from a string manifest
async fn start_actor_from_string(content: Option<String>, address: &str) -> Result<()> {
    // Try to resolve manifest content
    let manifest_content = match content {
        Some(content) => content,
        None => {
            return Err(anyhow::anyhow!("No manifest content provided"));
        }
    };

    // Validate that it's valid TOML before sending to the server
    match toml::from_str::<toml::Value>(&manifest_content) {
        Ok(_) => {
            println!(
                "{} Manifest content validated as valid TOML",
                style("INFO:").blue().bold()
            );
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Invalid TOML content: {}", e));
        }
    }

    // Pass the manifest content to the legacy command
    super::legacy::execute_command(
        super::legacy::Commands::StartFromString {
            manifest: manifest_content,
        },
        address,
    )
    .await
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
    let mut framed = connect_to_server(address).await?;

    println!(
        "{} Fetching details for actor {}",
        style("INFO:").blue().bold(),
        style(actor_id.clone()).green()
    );

    // Get actor status
    let command = ManagementCommand::GetActorStatus {
        id: actor_id.clone(),
    };
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    let actor_status;
    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorStatus { id: _, status } => {
                actor_status = Some(status);
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
    } else {
        println!(
            "{} No response received from server",
            style("Error:").red().bold()
        );
        return Ok(());
    }

    // Get actor state
    let command = ManagementCommand::GetActorState {
        id: actor_id.clone(),
    };
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    let mut state_size = None;
    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorState { id: _, state } => {
                state_size = state.map(|s| s.len());
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
    }

    // Get actor metrics
    let command = ManagementCommand::GetActorMetrics {
        id: actor_id.clone(),
    };
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    let mut metrics = None;
    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorMetrics { id: _, metrics: m } => {
                metrics = Some(m);
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
    }

    // Get actor events count
    let command = ManagementCommand::GetActorEvents {
        id: actor_id.clone(),
    };
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    let mut events_count = None;
    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorEvents { id: _, events } => {
                events_count = Some(events.len());
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
    }

    // Display gathered information
    println!("\n{}", style("Actor Details").bold().underlined());
    println!("ID:           {}", style(&actor_id).green());

    if let Some(status) = actor_status {
        println!("Status:       {}", style(format!("{:?}", status)).yellow());
    } else {
        println!("Status:       {}", style("Unknown").dim());
    }

    if let Some(size) = state_size {
        println!("State Size:   {} bytes", style(size).yellow());
    } else {
        println!("State:        {}", style("None").dim());
    }

    if let Some(count) = events_count {
        println!("Event Count:  {}", style(count).yellow());
    }

    if let Some(m) = metrics {
        println!("\n{}", style("Actor Metrics").bold().underlined());
        match serde_json::to_string_pretty(&m) {
            Ok(pretty) => {
                for line in pretty.lines() {
                    println!("  {}", line);
                }
            }
            Err(_) => {
                println!("  {}", style("Failed to format metrics").dim());
            }
        }
    }

    Ok(())
}

async fn restart_actor(id_opt: Option<String>, address: &str) -> Result<()> {
    println!("{}", style("Theater Actor Restart").bold().cyan());

    let actor_id = select_actor(id_opt, address).await?;
    let mut framed = connect_to_server(address).await?;

    println!(
        "{} Restarting actor {}",
        style("INFO:").blue().bold(),
        style(actor_id.clone()).green()
    );

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message("Restarting actor...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    // Send restart command
    let command = ManagementCommand::RestartActor {
        id: actor_id.clone(),
    };
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::Restarted { id } => {
                spinner.finish_with_message(format!(
                    "Actor {} restarted successfully",
                    style(id).green()
                ));
            }
            ManagementResponse::Error { message } => {
                spinner.finish_with_message(format!("{}", style("Restart failed").red()));
                println!("{} {}", style("Error:").red().bold(), message);
            }
            _ => {
                spinner.finish_with_message(format!("{}", style("Unexpected response").red()));
                println!(
                    "{} Unexpected response from server",
                    style("Error:").red().bold()
                );
            }
        }
    } else {
        spinner.finish_with_message(format!("{}", style("No response from server").red()));
        println!(
            "{} No response received from server",
            style("Error:").red().bold()
        );
    }

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
    let mut framed = connect_to_server(address).await?;

    println!(
        "{} Fetching logs for actor {}",
        style("INFO:").blue().bold(),
        style(actor_id.clone()).green()
    );

    // Get historical events first
    let command = ManagementCommand::GetActorEvents {
        id: actor_id.clone(),
    };
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    let events;
    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorEvents {
                id: _,
                events: actor_events,
            } => {
                events = actor_events;
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
    } else {
        println!(
            "{} No response received from server",
            style("Error:").red().bold()
        );
        return Ok(());
    }

    // Display the latest events first, up to the requested number of lines
    let event_count = events.len();
    let start_idx = if event_count > lines {
        event_count - lines
    } else {
        0
    };

    println!("\n{}", style("Actor Event Log").bold().underlined());

    if event_count == 0 {
        println!("No events recorded for this actor");
    } else {
        println!(
            "Showing {} of {} total events",
            style(event_count.min(lines)).yellow(),
            style(event_count).yellow()
        );

        for (i, event) in events.iter().skip(start_idx).enumerate() {
            // Format the event nicely
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let event_type = format!("{}", event);

            println!(
                "[{}] {}: {}",
                style(timestamp).dim(),
                style(i + 1).blue(),
                event_type
            );
        }
    }

    // If follow mode is enabled, subscribe to new events
    if follow {
        println!(
            "\n{} Following new events. Press Ctrl+C to stop.",
            style("INFO:").blue().bold()
        );

        // Subscribe to actor events
        let command = ManagementCommand::SubscribeToActor {
            id: actor_id.clone(),
        };
        let cmd_bytes = serde_json::to_vec(&command)?;
        framed.send(Bytes::from(cmd_bytes)).await?;

        let mut subscription_id = None;
        if let Some(Ok(data)) = framed.next().await {
            let response: ManagementResponse = serde_json::from_slice(&data)?;

            match response {
                ManagementResponse::Subscribed {
                    id: _,
                    subscription_id: sub_id,
                } => {
                    subscription_id = Some(sub_id);
                }
                _ => {
                    println!(
                        "{} Failed to subscribe to events",
                        style("Error:").red().bold()
                    );
                    return Ok(());
                }
            }
        }

        // Listen for events until user presses Ctrl+C
        let mut event_counter = events.len();
        loop {
            tokio::select! {
                Some(Ok(data)) = framed.next() => {
                    let response: ManagementResponse = serde_json::from_slice(&data)?;

                    if let ManagementResponse::ActorEvent { id: _, event } = response {
                        event_counter += 1;
                        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                        let event_type = format!("{:?}", event);

                        println!(
                            "[{}] {}: {}",
                            style(timestamp).dim(),
                            style(event_counter).blue(),
                            event_type
                        );
                    }
                },
                _ = tokio::signal::ctrl_c() => {
                    println!("\n{} Stopping event subscription", style("INFO:").blue().bold());

                    // Unsubscribe if we have a subscription ID
                    if let Some(sub_id) = subscription_id {
                        let command = ManagementCommand::UnsubscribeFromActor {
                            id: actor_id.clone(),
                            subscription_id: sub_id,
                        };
                        let cmd_bytes = serde_json::to_vec(&command)?;
                        framed.send(Bytes::from(cmd_bytes)).await?;
                    }

                    break;
                }
            }
        }
    }

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
    let json_value = match serde_json::from_str::<serde_json::Value>(&payload_data) {
        Ok(value) => value,
        Err(e) => {
            return Err(anyhow::anyhow!("Invalid JSON payload: {}", e));
        }
    };

    // Determine whether this is a request (expects response) or a send (fire-and-forget)
    let is_request = method.to_lowercase().starts_with("get")
        || method.to_lowercase().contains("request")
        || method.to_lowercase().contains("query");

    // Create a message that includes the method name and payload
    let mut message = std::collections::HashMap::new();
    message.insert(
        "method".to_string(),
        serde_json::Value::String(method.to_string()),
    );
    message.insert("payload".to_string(), json_value);

    // Serialize the message
    let message_bytes = serde_json::to_vec(&message)?;

    // Connect to server
    let mut framed = connect_to_server(address).await?;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );

    if is_request {
        spinner.set_message(format!("Requesting '{}' from actor...", method));
        spinner.enable_steady_tick(Duration::from_millis(80));

        // Send request command
        let command = ManagementCommand::RequestActorMessage {
            id: actor_id.clone(),
            data: message_bytes,
        };
        let cmd_bytes = serde_json::to_vec(&command)?;
        framed.send(Bytes::from(cmd_bytes)).await?;

        if let Some(Ok(data)) = framed.next().await {
            let response: ManagementResponse = serde_json::from_slice(&data)?;

            match response {
                ManagementResponse::RequestedMessage {
                    id: _,
                    message: response_bytes,
                } => {
                    spinner.finish_with_message(format!(
                        "Received response from actor's '{}' method",
                        style(method).green()
                    ));

                    // Try to parse response as JSON
                    match serde_json::from_slice::<serde_json::Value>(&response_bytes) {
                        Ok(json) => {
                            println!("\n{}", style("Response:").bold().underlined());
                            println!("{}", serde_json::to_string_pretty(&json)?);
                        }
                        Err(_) => {
                            println!("\n{}", style("Raw Response:").bold().underlined());
                            println!("Size: {} bytes", style(response_bytes.len()).yellow());

                            // If small enough, show as string
                            if response_bytes.len() < 1000 {
                                if let Ok(text) = String::from_utf8(response_bytes.clone()) {
                                    println!("\nContent: {}", text);
                                }
                            } else {
                                println!("\nBinary data too large to display");
                            }
                        }
                    }
                }
                ManagementResponse::Error { message } => {
                    spinner.finish_with_message(format!("{}", style("Request failed").red()));
                    println!("{} {}", style("Error:").red().bold(), message);
                }
                _ => {
                    spinner.finish_with_message(format!("{}", style("Unexpected response").red()));
                    println!(
                        "{} Unexpected response from server",
                        style("Error:").red().bold()
                    );
                }
            }
        } else {
            spinner.finish_with_message(format!("{}", style("No response from server").red()));
            println!(
                "{} No response received from server",
                style("Error:").red().bold()
            );
        }
    } else {
        // Send message (fire-and-forget)
        spinner.set_message(format!("Sending '{}' to actor...", method));
        spinner.enable_steady_tick(Duration::from_millis(80));

        // Send message command
        let command = ManagementCommand::SendActorMessage {
            id: actor_id.clone(),
            data: message_bytes,
        };
        let cmd_bytes = serde_json::to_vec(&command)?;
        framed.send(Bytes::from(cmd_bytes)).await?;

        if let Some(Ok(data)) = framed.next().await {
            let response: ManagementResponse = serde_json::from_slice(&data)?;

            match response {
                ManagementResponse::SentMessage { id: _ } => {
                    spinner.finish_with_message(format!(
                        "Message '{}' sent successfully to actor",
                        style(method).green()
                    ));
                }
                ManagementResponse::Error { message } => {
                    spinner.finish_with_message(format!("{}", style("Send failed").red()));
                    println!("{} {}", style("Error:").red().bold(), message);
                }
                _ => {
                    spinner.finish_with_message(format!("{}", style("Unexpected response").red()));
                    println!(
                        "{} Unexpected response from server",
                        style("Error:").red().bold()
                    );
                }
            }
        } else {
            spinner.finish_with_message(format!("{}", style("No response from server").red()));
            println!(
                "{} No response received from server",
                style("Error:").red().bold()
            );
        }
    }

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
    let mut framed = connect_to_server(address).await?;

    println!(
        "{} Fetching state for actor {}",
        style("INFO:").blue().bold(),
        style(actor_id.clone()).green()
    );

    // Get actor state
    let command = ManagementCommand::GetActorState {
        id: actor_id.clone(),
    };
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;

        match response {
            ManagementResponse::ActorState {
                id: _,
                state: Some(state_bytes),
            } => {
                // Try to parse the state as JSON
                let state_json = match serde_json::from_slice::<serde_json::Value>(&state_bytes) {
                    Ok(json) => json,
                    Err(_) => {
                        // If not valid JSON, just show as raw data size
                        println!("\n{}", style("Raw State Data").bold().underlined());
                        println!("Size: {} bytes", style(state_bytes.len()).yellow());
                        println!(
                            "\nState is binary data (not JSON). Use export option to save to file."
                        );

                        // Export if requested
                        if let Some(path) = export {
                            fs::write(&path, &state_bytes).await?;
                            println!(
                                "\n{} State exported to {}",
                                style("✓").green().bold(),
                                style(path.display()).green()
                            );
                        }

                        return Ok(());
                    }
                };

                // Format based on user preference
                let formatted_state = match format.to_lowercase().as_str() {
                    "json" => serde_json::to_string_pretty(&state_json)?,
                    "yaml" => serde_yaml::to_string(&state_json)?,
                    "toml" => {
                        println!(
                            "{} TOML format not fully supported, using JSON",
                            style("Note:").yellow().bold()
                        );
                        serde_json::to_string_pretty(&state_json)?
                    }
                    _ => serde_json::to_string_pretty(&state_json)?,
                };

                // Display the state
                println!("\n{}", style("Actor State").bold().underlined());
                println!("{}", formatted_state);

                // Export if requested
                if let Some(path) = export {
                    fs::write(&path, formatted_state).await?;
                    println!(
                        "\n{} State exported to {}",
                        style("✓").green().bold(),
                        style(path.display()).green()
                    );
                }
            }
            ManagementResponse::ActorState { id: _, state: None } => {
                println!(
                    "\n{} Actor has no state data",
                    style("Note:").yellow().bold()
                );
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
