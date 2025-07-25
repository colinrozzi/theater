use anyhow::Result;
use clap::Parser;
use console::style;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::{error::CliError, output::formatters::ChannelOpened, CommandContext};
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct OpenArgs {
    /// ID of the actor to open a channel with
    #[arg(required = true)]
    pub actor_id: String,

    /// Initial message to send when opening the channel
    #[arg(short, long)]
    pub message: Option<String>,

    /// File containing initial message to send
    #[arg(short, long, conflicts_with = "message")]
    pub file: Option<PathBuf>,

    /// Address of the theater server
    #[arg(short, long)]
    pub address: Option<SocketAddr>,
}

/// Execute the channel open command asynchronously (modernized)
pub async fn execute_async(args: &OpenArgs, ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Opening channel to actor: {}", args.actor_id);

    // Get initial message content either from direct argument or file
    let initial_message = if let Some(message) = &args.message {
        message.clone().into_bytes()
    } else if let Some(file_path) = &args.file {
        debug!("Reading initial message from file: {:?}", file_path);
        fs::read(file_path).map_err(|e| {
            CliError::file_operation_failed("read message file", file_path.display().to_string(), e)
        })?
    } else {
        // Default initial message
        serde_json::to_vec(&serde_json::json!({
            "message_type": "channel_init",
            "payload": {
                "timestamp": chrono::Utc::now().timestamp_millis(),
            }
        }))
        .map_err(|e| {
            CliError::invalid_input(
                "initial_message",
                "json",
                format!("Failed to create default initial message: {}", e),
            )
        })?
    };

    debug!("Initial message size: {} bytes", initial_message.len());

    // Parse the actor ID
    let actor_id = TheaterId::parse(&args.actor_id)
        .map_err(|_e| CliError::invalid_actor_id(&args.actor_id))?;

    // Get server address from args or config
    let address = ctx.server_address(args.address);
    debug!("Connecting to server at: {}", address);

    // Run the interactive channel session
    run_channel_session(address, actor_id, initial_message, ctx).await
}

async fn run_channel_session(
    server_addr: SocketAddr,
    actor_id: TheaterId,
    initial_message: Vec<u8>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    // Create client and connect
    let client = ctx.create_client();
    client
        .connect()
        .await
        .map_err(|e| CliError::connection_failed(server_addr, e))?;

    // Capture initial message size before move
    let initial_message_size = initial_message.len();

    // Open a channel to the actor
    let channel_id = client
        .open_channel(&actor_id.to_string(), initial_message)
        .await
        .map_err(|e| {
            CliError::actor_not_found(format!(
                "Failed to open channel to actor {}: {}",
                actor_id, e
            ))
        })?;
    // Create channel info for output
    let channel_info = ChannelOpened {
        actor_id: actor_id.clone(),
        channel_id: channel_id.clone(),
        address: server_addr.to_string(),
        initial_message_size,
        is_interactive: !ctx.json,
    };

    // Display channel open info
    ctx.output.output(&channel_info, None)?;

    // If JSON mode, we don't run interactive mode
    if ctx.json {
        return Ok(());
    }

    // Set up the REPL for interactive mode
    let mut rl = DefaultEditor::new().map_err(|e| {
        CliError::invalid_input(
            "readline",
            "setup",
            format!("Failed to setup readline: {}", e),
        )
    })?;

    println!(
        "{} Enter commands ('help' for available commands, 'exit' to quit)",
        style("i").blue().bold()
    );

    // Create a task to listen for input
    let (input_tx, mut input_rx) = mpsc::channel::<String>(32);
    let input_task = tokio::spawn(async move {
        loop {
            let readline = rl.readline("channel> ");
            match readline {
                Ok(line) => {
                    let _ = rl.add_history_entry(line.as_str());
                    if let Err(_) = input_tx.send(line).await {
                        break;
                    }
                }
                Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                    println!("\rClosing channel and exiting...");
                    break;
                }
                Err(err) => {
                    println!("\rError: {}", err);
                    break;
                }
            }
        }
    });

    // Main event loop using select!
    let mut running = true;

    while running {
        tokio::select! {
            // Handle user input
            Some(line) = input_rx.recv() => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Process commands
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                let cmd = parts[0].to_lowercase();

                match cmd.as_str() {
                    "send" => {
                        // Handle send command with various formats
                        if parts.len() < 2 {
                            println!("Error: send requires a message or --file option");
                            continue;
                        }

                        let message = if parts[1] == "--file" || parts[1] == "-f" {
                            println!("{} Reading message from file...", style(">").green().bold());
                            if parts.len() < 3 {
                                println!("Error: --file option requires a file path");
                                continue;
                            }

                            let file_path = parts[2];
                            match fs::read(file_path) {
                                Ok(content) => {
                                    println!("{} Read {} bytes from file",
                                        style("✓").green().bold(), content.len());
                                    content
                                }
                                Err(e) => {
                                    println!("Error reading file: {}", e);
                                    continue;
                                }
                            }
                        } else {
                            // Send the rest of the line as the message
                            let message_text = trimmed[5..].trim(); // Skip "send "

                            // Check if it's a quoted string and remove the quotes if needed
                            let text = if message_text.starts_with('"')
                                && message_text.ends_with('"')
                                && message_text.len() >= 2
                            {
                                &message_text[1..message_text.len() - 1]
                            } else {
                                message_text
                            };

                            text.as_bytes().to_vec()
                        };

                        debug!("Sending message on channel: {} bytes", message.len());

                        // Send the message
                        match client.send_on_channel(&channel_id, message).await {
                            Ok(_) => {
                                if ctx.verbose {
                                    println!("{} Message sent", style("✓").green().bold());
                                }
                            }
                            Err(e) => {
                                println!("{} Error sending message: {}",
                                    style("✗").red().bold(), e);
                            }
                        }
                    }
                    "exit" | "quit" => {
                        running = false;
                        println!("Closing channel and exiting...");
                    }
                    "help" => {
                        println!("Available commands:");
                        println!("  send \"message\"    - Send a text message");
                        println!("  send --file path  - Send contents of a file");
                        println!("  exit | quit       - Close channel and exit");
                        println!("  help              - Show this help");
                    }
                    _ => {
                        println!("Unknown command: {}. Type 'help' for available commands.", cmd);
                    }
                }
            },
            // Handle incoming messages
            result = client.receive_channel_message() => {
                match result {
                    Ok(response) => {
                        if let Some((id, message)) = response {
                            // Only process messages for our channel
                            if id == channel_id {
                                // Try to pretty print if it looks like JSON
                                match std::str::from_utf8(&message) {
                                    Ok(text) => {
                                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                                            println!("\r{}", serde_json::to_string_pretty(&json)
                                                .unwrap_or_else(|_| text.to_string()));
                                        } else {
                                            println!("\r{}", text);
                                        }
                                    },
                                    Err(_) => {
                                        println!("\r[Binary message of {} bytes]", message.len());
                                        if ctx.verbose {
                                            println!("\r{:?}", message);
                                        }
                                    }
                                }
                                // Re-display the prompt
                                print!("channel> ");
                                let _ = std::io::Write::flush(&mut std::io::stdout());
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error receiving channel message: {}", e);
                        // Short backoff before retrying
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }
    }

    // Clean up
    input_task.abort();

    // Close the channel
    client
        .close_channel(&channel_id)
        .await
        .map_err(|e| CliError::actor_not_found(format!("Failed to close channel: {}", e)))?;

    println!("{} Channel closed", style("✓").green().bold());

    Ok(())
}

/// Legacy wrapper for backward compatibility
pub fn execute(args: &OpenArgs, verbose: bool, json: bool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let output = crate::output::OutputManager::new(config.output.clone());
        let shutdown_token = tokio_util::sync::CancellationToken::new();
        let ctx = crate::CommandContext {
            config,
            output,
            verbose,
            json,
            shutdown_token,
        };
        execute_async(args, &ctx)
            .await
            .map_err(|e| anyhow::Error::from(e))
    })
}
