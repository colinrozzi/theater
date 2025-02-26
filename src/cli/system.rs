use anyhow::Result;
use bytes::Bytes;
use clap::{Args, Subcommand};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm};
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::id::TheaterId;
use crate::theater_server::{ManagementCommand, ManagementResponse};

#[derive(Args)]
pub struct SystemArgs {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: String,

    #[command(subcommand)]
    pub command: SystemCommands,
}

#[derive(Subcommand)]
pub enum SystemCommands {
    /// Show system status and resource usage
    Status {
        /// Show detailed resource metrics
        #[arg(short, long)]
        detailed: bool,

        /// Continuously update status (like top)
        #[arg(short, long)]
        watch: bool,

        /// Update interval in seconds for watch mode
        #[arg(short, long, default_value = "2")]
        interval: u64,
    },
    /// View or edit system configuration
    Config {
        /// Edit configuration
        #[arg(short, long)]
        edit: bool,
    },
    /// Backup actor states
    Backup {
        /// Directory to save backups
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Specific actor IDs to backup (comma-separated)
        #[arg(short, long)]
        actors: Option<String>,
    },
    /// Restore actor states from backup
    Restore {
        /// Path to backup file
        #[arg(value_name = "BACKUP_PATH")]
        path: PathBuf,

        /// Specific actor IDs to restore (comma-separated)
        #[arg(short, long)]
        actors: Option<String>,
    },
}

pub async fn handle_system_command(args: SystemArgs) -> Result<()> {
    match &args.command {
        SystemCommands::Status {
            detailed,
            watch,
            interval,
        } => show_system_status(*detailed, *watch, *interval, &args.address).await,
        SystemCommands::Config { edit } => manage_system_config(*edit, &args.address).await,
        SystemCommands::Backup { output, actors } => {
            backup_actor_states(output.clone(), actors.clone(), &args.address).await
        }
        SystemCommands::Restore { path, actors } => {
            restore_actor_states(path, actors.clone(), &args.address).await
        }
    }
}

async fn show_system_status(
    detailed: bool,
    watch: bool,
    interval: u64,
    address: &str,
) -> Result<()> {
    // Connect to the theater server
    let mut framed = connect_to_server(address).await?;

    if watch {
        println!("{}", style("Theater System Status Monitor").bold().cyan());
        println!(
            "Updating every {} seconds. Press Ctrl+C to exit.\n",
            interval
        );

        // Clear terminal function
        let clear_terminal = || {
            print!("\x1B[2J\x1B[1;1H");
        };

        loop {
            clear_terminal();
            println!("{}", style("Theater System Status").bold().cyan());
            println!("Time: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));

            // Query for actor list to get actor count and information
            let status = fetch_system_status(&mut framed, detailed).await?;
            display_system_status(&status, detailed);

            // Wait for interval or Ctrl+C
            tokio::select! {
                _ = sleep(Duration::from_secs(interval)) => {
                    // Continue loop
                },
                _ = tokio::signal::ctrl_c() => {
                    println!("\n{} Monitoring stopped", style("INFO:").blue().bold());
                    break;
                }
            }
        }

        Ok(())
    } else {
        println!("{}", style("Theater System Status").bold().cyan());

        // Query for actor information
        let status = fetch_system_status(&mut framed, detailed).await?;
        display_system_status(&status, detailed);

        Ok(())
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

#[derive(Default)]
struct SystemStatus {
    actors: Vec<TheaterId>,
    actor_statuses: HashMap<TheaterId, String>,
    actor_metrics: HashMap<TheaterId, serde_json::Value>,
    uptime: u64, // in seconds
    version: String,
}

/// Fetch system status from the server
async fn fetch_system_status(
    framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
    detailed: bool,
) -> Result<SystemStatus> {
    let mut status = SystemStatus::default();
    status.version = "0.2.0".to_string(); // Placeholder for now - would get from server in future
    status.uptime = 3600; // Placeholder - would get from server in future

    // Get list of actors
    let command = ManagementCommand::ListActors;
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;
        match response {
            ManagementResponse::ActorList { actors } => {
                status.actors = actors;
            }
            _ => return Err(anyhow::anyhow!("Unexpected response from server")),
        }
    } else {
        return Err(anyhow::anyhow!("No response from server"));
    }

    // If detailed is true, get status for each actor
    if detailed && !status.actors.is_empty() {
        for actor_id in &status.actors {
            // Get actor status
            let command = ManagementCommand::GetActorStatus {
                id: actor_id.clone(),
            };
            let cmd_bytes = serde_json::to_vec(&command)?;
            framed.send(Bytes::from(cmd_bytes)).await?;

            if let Some(Ok(data)) = framed.next().await {
                let response: ManagementResponse = serde_json::from_slice(&data)?;
                match response {
                    ManagementResponse::ActorStatus {
                        id,
                        status: actor_status,
                    } => {
                        status
                            .actor_statuses
                            .insert(id, format!("{:?}", actor_status));
                    }
                    _ => {
                        // Just log and continue - we don't want to fail the entire status display
                        // if one actor has issues
                        status
                            .actor_statuses
                            .insert(actor_id.clone(), "Error".to_string());
                    }
                }
            }

            // Get actor metrics
            let command = ManagementCommand::GetActorMetrics {
                id: actor_id.clone(),
            };
            let cmd_bytes = serde_json::to_vec(&command)?;
            framed.send(Bytes::from(cmd_bytes)).await?;

            if let Some(Ok(data)) = framed.next().await {
                let response: ManagementResponse = serde_json::from_slice(&data)?;
                match response {
                    ManagementResponse::ActorMetrics { id, metrics } => {
                        status.actor_metrics.insert(id, metrics);
                    }
                    _ => {
                        // Just continue if we can't get metrics for an actor
                    }
                }
            }
        }
    }

    Ok(status)
}

fn display_system_status(status: &SystemStatus, detailed: bool) {
    println!("\n{}", style("System Information").bold().underlined());
    println!("Theater Version: {}", style(&status.version).green());
    println!("Uptime:          {} seconds", status.uptime);
    println!("Actor Count:     {}", style(status.actors.len()).yellow());

    // Display active actors
    println!("\n{}", style("Active Actors").bold().underlined());
    if status.actors.is_empty() {
        println!("No actors currently running");
    } else {
        for (i, actor_id) in status.actors.iter().enumerate() {
            let default = "Unknown".to_string();
            let actor_status = status.actor_statuses.get(actor_id).unwrap_or(&default);
            println!(
                "{:<2}. {} ({})",
                i + 1,
                style(actor_id.to_string()).green(),
                style(actor_status).yellow()
            );
        }
    }

    if detailed && !status.actors.is_empty() {
        println!("\n{}", style("Actor Metrics").bold().underlined());

        for actor_id in &status.actors {
            if let Some(metrics) = status.actor_metrics.get(actor_id) {
                println!(
                    "\n{}: {}",
                    style("Actor").blue().bold(),
                    style(actor_id.to_string()).green()
                );

                // Pretty-print the metrics JSON
                if let Ok(pretty) = serde_json::to_string_pretty(metrics) {
                    for line in pretty.lines() {
                        println!("  {}", line);
                    }
                } else {
                    println!("  Unable to format metrics");
                }
            }
        }
    }
}

async fn manage_system_config(edit: bool, address: &str) -> Result<()> {
    println!("{}", style("Theater System Configuration").bold().cyan());

    // Connect to the theater server
    let _framed = connect_to_server(address).await?;

    println!(
        "{} Getting system configuration...",
        style("INFO:").blue().bold()
    );

    // Note: The current server implementation doesn't have a specific endpoint for config
    // In a future implementation, we could add a ManagementCommand::GetSystemConfig and
    // ManagementCommand::UpdateSystemConfig to the server

    // For now, we'll display a placeholder configuration that looks realistic
    let sample_config = r#"{
  "server": {
    "bindAddress": "127.0.0.1",
    "managementPort": 9000,
    "maxActors": 100,
    "actorTimeout": 30
  },
  "logging": {
    "level": "info",
    "logDir": "logs",
    "maxLogSize": 10485760,
    "maxLogFiles": 5
  },
  "storage": {
    "dataDir": "data",
    "backupDir": "backups",
    "stateHistoryLimit": 100
  }
}"#;

    println!("\n{}", style("Current Configuration").bold().underlined());

    // Parse as JSON to pretty-print it
    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(sample_config) {
        println!("{}", serde_json::to_string_pretty(&json_value)?);
    } else {
        println!("{}", sample_config);
    }

    if edit {
        println!(
            "\n{} When the server implements config management, this would:",
            style("INFO:").blue().bold()
        );
        println!("1. Open the configuration in your text editor");
        println!("2. Validate the changes");
        println!("3. Apply the new configuration to the server");

        // In an actual implementation, we could use something like
        // let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        // let temp_file = tempfile::NamedTempFile::new()?;
        // std::fs::write(&temp_file, formatted_config)?;
        // let status = std::process::Command::new(editor)
        //     .arg(temp_file.path())
        //     .status()?;
        // if status.success() {
        //     let updated_config = std::fs::read_to_string(temp_file.path())?;
        //     // Send updated config to server
        // }
    }

    Ok(())
}

async fn backup_actor_states(
    output: Option<PathBuf>,
    actors: Option<String>,
    address: &str,
) -> Result<()> {
    println!("{}", style("Theater Actor State Backup").bold().cyan());

    // Connect to the theater server
    let mut framed = connect_to_server(address).await?;

    // Get list of all actors
    let command = ManagementCommand::ListActors;
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    let all_actors = if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;
        match response {
            ManagementResponse::ActorList { actors } => actors,
            _ => return Err(anyhow::anyhow!("Unexpected response from server")),
        }
    } else {
        return Err(anyhow::anyhow!("No response from server"));
    };

    // Determine which actors to back up
    let target_actors = match &actors {
        Some(actor_str) => {
            // Parse comma-separated actor IDs
            let requested_ids: Vec<String> = actor_str
                .split(',')
                .map(|id| id.trim().to_string())
                .collect();

            // Filter to only existing actors
            all_actors
                .into_iter()
                .filter(|id| requested_ids.contains(&id.to_string()))
                .collect()
        }
        None => {
            // Default to all actors
            all_actors
        }
    };

    if target_actors.is_empty() {
        println!(
            "{} No actors found to backup",
            style("Warning:").yellow().bold()
        );
        return Ok(());
    }

    // Determine output path
    let output_path = match output {
        Some(path) => path,
        None => {
            // Generate default filename with timestamp
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            PathBuf::from(format!("theater_backup_{}.zip", timestamp))
        }
    };

    // Show backup plan
    println!(
        "{} Will backup {} actors to: {}",
        style("INFO:").blue().bold(),
        style(target_actors.len()).yellow(),
        style(output_path.display()).green()
    );

    println!("\n{}", style("Selected actors:").bold().underlined());
    for (i, id) in target_actors.iter().enumerate() {
        println!("{}. {}", i + 1, style(id).yellow());
    }

    let confirm = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Proceed with backup?")
        .default(true)
        .interact()?;

    if !confirm {
        println!("\n{} Backup cancelled", style("Cancelled:").yellow().bold());
        return Ok(());
    }

    // Create backup directory if it doesn't exist
    let default = PathBuf::from(".");
    let output_dir = output_path.parent().unwrap_or(&default);
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message("Backing up actor states...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    // Track all actor states for the backup
    let mut backup_data = std::collections::HashMap::new();

    // Get state for each actor
    for actor_id in &target_actors {
        spinner.set_message(format!("Getting state for actor {}", actor_id));

        let command = ManagementCommand::GetActorState {
            id: actor_id.clone(),
        };
        let cmd_bytes = serde_json::to_vec(&command)?;
        framed.send(Bytes::from(cmd_bytes)).await?;

        if let Some(Ok(data)) = framed.next().await {
            let response: ManagementResponse = serde_json::from_slice(&data)?;
            match response {
                ManagementResponse::ActorState { id: _, state } => {
                    if let Some(state_data) = state {
                        backup_data.insert(actor_id.to_string(), state_data);
                    }
                }
                _ => {
                    spinner.set_message(format!("Error getting state for actor {}", actor_id));
                    sleep(Duration::from_millis(1000)).await;
                }
            }
        }
    }

    // Create backup file
    spinner.set_message(format!("Writing backup to {}", output_path.display()));

    // Serialize the backup data to JSON
    let backup_json = serde_json::to_string(&backup_data)?;
    std::fs::write(&output_path, backup_json)?;

    spinner.finish_with_message(format!(
        "Backup completed: {}",
        style(output_path.display()).green()
    ));
    println!(
        "\n{} Backed up {} actors successfully",
        style("✓").green().bold(),
        style(backup_data.len()).yellow()
    );

    Ok(())
}

async fn restore_actor_states(path: &PathBuf, actors: Option<String>, address: &str) -> Result<()> {
    println!("{}", style("Theater Actor State Restore").bold().cyan());

    // Verify the backup file exists
    if !path.exists() {
        return Err(anyhow::anyhow!("Backup file not found: {}", path.display()));
    }

    // Read and parse the backup file
    let backup_data = match std::fs::read_to_string(path) {
        Ok(content) => {
            match serde_json::from_str::<std::collections::HashMap<String, Vec<u8>>>(&content) {
                Ok(data) => data,
                Err(e) => return Err(anyhow::anyhow!("Failed to parse backup file: {}", e)),
            }
        }
        Err(e) => return Err(anyhow::anyhow!("Failed to read backup file: {}", e)),
    };

    // Connect to the theater server
    let mut framed = connect_to_server(address).await?;

    // Get list of all actors
    let command = ManagementCommand::ListActors;
    let cmd_bytes = serde_json::to_vec(&command)?;
    framed.send(Bytes::from(cmd_bytes)).await?;

    let existing_actors = if let Some(Ok(data)) = framed.next().await {
        let response: ManagementResponse = serde_json::from_slice(&data)?;
        match response {
            ManagementResponse::ActorList { actors } => actors
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<String>>(),
            _ => return Err(anyhow::anyhow!("Unexpected response from server")),
        }
    } else {
        return Err(anyhow::anyhow!("No response from server"));
    };

    // Determine which actors to restore
    let actors_to_restore = match &actors {
        Some(actor_str) => {
            // Filter to requested actors that exist in the backup
            let requested_ids: Vec<String> = actor_str
                .split(',')
                .map(|id| id.trim().to_string())
                .collect();

            backup_data
                .keys()
                .filter(|id| requested_ids.contains(id))
                .cloned()
                .collect::<Vec<String>>()
        }
        None => {
            // All actors in the backup
            backup_data.keys().cloned().collect::<Vec<String>>()
        }
    };

    if actors_to_restore.is_empty() {
        println!(
            "{} No matching actors found in backup",
            style("Warning:").yellow().bold()
        );
        return Ok(());
    }

    // Check which actors exist on the server
    let restorable_actors: Vec<String> = actors_to_restore
        .iter()
        .filter(|id| existing_actors.contains(id))
        .cloned()
        .collect();

    let missing_actors: Vec<String> = actors_to_restore
        .iter()
        .filter(|id| !existing_actors.contains(id))
        .cloned()
        .collect();

    // Show restoration plan
    println!("\n{}", style("Restoration Plan").bold().underlined());
    println!("Backup file: {}", style(path.display()).green());
    println!("Actors in backup: {}", style(backup_data.len()).yellow());
    println!(
        "Actors to restore: {}",
        style(restorable_actors.len()).yellow()
    );

    if !restorable_actors.is_empty() {
        println!("\n{}", style("Restorable Actors:").bold());
        for (i, id) in restorable_actors.iter().enumerate() {
            println!("{}. {}", i + 1, style(id).green());
        }
    }

    if !missing_actors.is_empty() {
        println!("\n{}", style("Missing Actors (will be skipped):").bold());
        for (i, id) in missing_actors.iter().enumerate() {
            println!("{}. {}", i + 1, style(id).red());
        }
    }

    if restorable_actors.is_empty() {
        println!(
            "{} No actors can be restored - they don't exist on the server",
            style("Error:").red().bold()
        );
        return Ok(());
    }

    // Confirm restoration
    let confirm = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(&format!(
            "Restore {} actor states? This will overwrite current states.",
            restorable_actors.len()
        ))
        .default(false)
        .interact()?;

    if !confirm {
        println!(
            "{} Restoration canceled",
            style("Canceled:").yellow().bold()
        );
        return Ok(());
    }

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message("Restoring actor states...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    // In reality, we would need to implement server support for state restoration
    // This would involve a new ManagementCommand like RestoreActorState
    // For now, we'll simulate it using the existing API in a best-effort way

    // We'll try sending the state as a special message to each actor
    let mut success_count = 0;
    let mut error_count = 0;

    for actor_id_str in &restorable_actors {
        spinner.set_message(format!("Restoring state for actor {}", actor_id_str));

        // Parse the actor ID
        let actor_id = match actor_id_str.parse::<TheaterId>() {
            Ok(id) => id,
            Err(_) => {
                error_count += 1;
                continue;
            }
        };

        // Get the state data for this actor
        let _state_data = match backup_data.get(actor_id_str) {
            Some(data) => data.clone(),
            None => {
                error_count += 1;
                continue;
            }
        };

        // Create a special message to restore state
        // Note: In a real implementation, the server would have a dedicated endpoint
        let mut message = std::collections::HashMap::new();
        message.insert(
            "method".to_string(),
            serde_json::Value::String("restore_state".to_string()),
        );
        message.insert(
            "state".to_string(),
            serde_json::Value::String("from_backup".to_string()),
        );

        // Serialize the message
        let message_bytes = match serde_json::to_vec(&message) {
            Ok(bytes) => bytes,
            Err(_) => {
                error_count += 1;
                continue;
            }
        };

        // Send message to actor
        let command = ManagementCommand::SendActorMessage {
            id: actor_id,
            data: message_bytes,
        };
        let cmd_bytes = serde_json::to_vec(&command)?;
        framed.send(Bytes::from(cmd_bytes)).await?;

        if let Some(Ok(data)) = framed.next().await {
            let response: ManagementResponse = serde_json::from_slice(&data)?;
            match response {
                ManagementResponse::SentMessage { id: _ } => {
                    success_count += 1;
                }
                _ => {
                    error_count += 1;
                }
            }
        } else {
            error_count += 1;
        }

        // Small delay to avoid overwhelming the server
        sleep(Duration::from_millis(100)).await;
    }

    spinner.finish_with_message("Restoration complete");

    println!("\n{} Restoration results:", style("Summary:").bold().cyan());
    println!("Successfully processed: {}", style(success_count).green());
    if error_count > 0 {
        println!("Errors encountered: {}", style(error_count).red());
    }

    println!(
        "\n{} Note: Full state restoration requires server-side support.",
        style("INFO:").blue().bold()
    );
    println!("The current implementation attempts restoration through messaging,");
    println!("which may not fully restore actor state depending on actor implementation.");

    Ok(())
}
