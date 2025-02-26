use crate::id::TheaterId;
use crate::theater_server::{ManagementCommand, ManagementResponse};
use anyhow::Result;
use bytes::Bytes;
use console::style;
use dialoguer::{theme::ColorfulTheme, Input, Select};
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::signal;
use tokio::time::sleep;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error};

// Legacy commands that match the original CLI structure
pub enum Commands {
    Start { manifest: Option<PathBuf> },
    Stop { id: Option<String> },
    List { detailed: bool },
    Subscribe { id: Option<String> },
}

pub async fn run_interactive_mode(address: &str) -> Result<()> {
    println!(
        "{}",
        style("Theater WebAssembly Actor System").bold().cyan()
    );
    println!("Connected to server at {}\n", style(address).yellow());

    loop {
        let options = vec![
            "Start Actor",
            "Stop Actor",
            "List Actors",
            "Subscribe to Events",
            "Exit",
        ];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Choose an action")
            .default(0)
            .items(&options)
            .interact()?;

        match selection {
            0 => {
                let manifest_str = Input::<String>::new()
                    .with_prompt("Enter manifest path")
                    .interact_text()?;
                let manifest = PathBuf::from(manifest_str);

                execute_command(
                    Commands::Start {
                        manifest: Some(manifest),
                    },
                    address,
                )
                .await?;
            }
            1 => {
                // First get the list of actors
                let actors = get_actor_list(address).await?;

                if actors.is_empty() {
                    println!("{}", style("No running actors found").red());
                    continue;
                }

                let actor_strings: Vec<String> = actors.iter().map(|id| id.to_string()).collect();

                let selection = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Select actor to stop")
                    .default(0)
                    .items(&actor_strings)
                    .interact()?;

                execute_command(
                    Commands::Stop {
                        id: Some(actor_strings[selection].clone()),
                    },
                    address,
                )
                .await?;
            }
            2 => {
                execute_command(Commands::List { detailed: true }, address).await?;
            }
            3 => {
                // First get the list of actors
                let actors = get_actor_list(address).await?;

                if actors.is_empty() {
                    println!("{}", style("No running actors found").red());
                    continue;
                }

                let actor_strings: Vec<String> = actors.iter().map(|id| id.to_string()).collect();

                let selection = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Select actor to monitor")
                    .default(0)
                    .items(&actor_strings)
                    .interact()?;

                execute_command(
                    Commands::Subscribe {
                        id: Some(actor_strings[selection].clone()),
                    },
                    address,
                )
                .await?;
            }
            4 => break,
            _ => unreachable!(),
        }

        println!();
    }

    Ok(())
}

async fn get_actor_list(address: &str) -> Result<Vec<TheaterId>> {
    let stream = TcpStream::connect(address).await?;
    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

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

pub async fn execute_command(command: Commands, address: &str) -> Result<()> {
    // Connect to the server
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

    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

    // Send command based on CLI args
    let server_command = match command {
        Commands::Start { manifest } => {
            let manifest_path = match manifest {
                Some(path) => path,
                None => {
                    let path_str = Input::<String>::new()
                        .with_prompt("Enter manifest path")
                        .interact_text()?;
                    PathBuf::from(path_str)
                }
            };

            spinner.set_message(format!("Starting actor from manifest: {:?}", manifest_path));

            ManagementCommand::StartActor {
                manifest: manifest_path,
            }
        }
        Commands::Stop { id } => {
            let actor_id = match id {
                Some(id_str) => id_str.parse()?,
                None => {
                    // Get actor list and prompt user to select one
                    spinner.finish();

                    let actors = get_actor_list(address).await?;
                    if actors.is_empty() {
                        println!("{}", style("No running actors found").red());
                        return Ok(());
                    }

                    let actor_strings: Vec<String> =
                        actors.iter().map(|id| id.to_string()).collect();

                    let selection = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("Select actor to stop")
                        .default(0)
                        .items(&actor_strings)
                        .interact()?;

                    actor_strings[selection].parse()?
                }
            };

            spinner.set_message(format!("Stopping actor: {}", actor_id));

            ManagementCommand::StopActor { id: actor_id }
        }
        Commands::List { detailed: _ } => {
            spinner.set_message("Fetching actor list");
            ManagementCommand::ListActors
        }
        Commands::Subscribe { id } => {
            let actor_id = match id {
                Some(id_str) => id_str.parse()?,
                None => {
                    // Get actor list and prompt user to select one
                    spinner.finish();

                    let actors = get_actor_list(address).await?;
                    if actors.is_empty() {
                        println!("{}", style("No running actors found").red());
                        return Ok(());
                    }

                    let actor_strings: Vec<String> =
                        actors.iter().map(|id| id.to_string()).collect();

                    let selection = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("Select actor to monitor")
                        .default(0)
                        .items(&actor_strings)
                        .interact()?;

                    actor_strings[selection].parse()?
                }
            };

            spinner.set_message(format!("Subscribing to actor: {}", actor_id));

            ManagementCommand::SubscribeToActor { id: actor_id }
        }
    };

    // Send the command
    debug!("Sending command: {:?}", server_command);
    let cmd_bytes = serde_json::to_vec(&server_command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;
    debug!("Command sent, waiting for response");

    spinner.finish();

    // Handle response(s)
    let is_subscribe = matches!(server_command, ManagementCommand::SubscribeToActor { .. });
    if is_subscribe {
        println!("{}", style("Event Monitor").bold().underlined());
        println!("Press Ctrl+C to stop monitoring\n");
    }

    let mut event_count = 0;
    while let Some(msg) = framed.next().await {
        match msg {
            Ok(data) => {
                let response: ManagementResponse = serde_json::from_slice(&data)?;
                match response {
                    ManagementResponse::ActorStarted { id } => {
                        println!(
                            "{} Actor started with ID: {}",
                            style("✓").green().bold(),
                            style(id).green()
                        );
                    }
                    ManagementResponse::ActorStopped { id } => {
                        println!(
                            "{} Actor {} stopped successfully",
                            style("✓").green().bold(),
                            style(id).yellow()
                        );
                    }
                    ManagementResponse::ActorList { actors } => {
                        if actors.is_empty() {
                            println!("No actors running");
                        } else {
                            println!("{}", style("Running Actors").bold().underlined());
                            for (i, actor) in actors.iter().enumerate() {
                                println!("{}. {}", i + 1, style(actor).green());
                            }
                        }
                    }
                    ManagementResponse::Subscribed {
                        id,
                        subscription_id,
                    } => {
                        println!(
                            "Monitoring actor {} (Subscription ID: {})",
                            style(id).green(),
                            style(subscription_id).dim()
                        );
                    }
                    ManagementResponse::ActorEvent { id, event } => {
                        event_count += 1;
                        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f");

                        println!(
                            "[{}] [{}] Event {}: {}",
                            style(timestamp).dim(),
                            style(&id).blue(),
                            style(event_count).yellow(),
                            style(&event.event_type).cyan()
                        );

                        if let Ok(json) = serde_json::to_string_pretty(&event.data) {
                            // Indent the JSON output
                            for line in json.lines() {
                                println!("  {}", line);
                            }
                            println!();
                        } else {
                            println!("  Data: {:?}\n", event.data);
                        }
                    }
                    ManagementResponse::Unsubscribed { id } => {
                        println!("Unsubscribed from actor {}", style(id).yellow());
                    }
                    ManagementResponse::Error { message } => {
                        println!("{} {}", style("Error:").red().bold(), message);
                    }
                    _ => {
                        println!(
                            "{} Unexpected response type from server",
                            style("Warning:").yellow().bold()
                        );
                    }
                }
            }
            Err(e) => {
                error!("Error receiving response: {}", e);
                println!("{} {}", style("Error:").red().bold(), e);
                break;
            }
        }

        // If not subscribed, break after first response
        if !is_subscribe {
            break;
        }

        // Check for Ctrl+C if in subscription mode
        if is_subscribe {
            if let Ok(()) = tokio::select! {
                _ = sleep(Duration::from_millis(10)) => { Err(()) }
                _ = signal::ctrl_c() => { Ok(()) }
            } {
                println!("\n{}", style("Event monitoring stopped").yellow());
                break;
            }
        }
    }

    Ok(())
}
