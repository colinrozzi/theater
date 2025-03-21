use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::cli::client::TheaterClient;
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
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

/// Execute the channel open command
pub fn execute(args: &OpenArgs, verbose: bool, json: bool) -> Result<()> {
    debug!("Opening channel to actor: {}", args.actor_id);

    // Get initial message content either from direct argument or file
    let initial_message = if let Some(message) = &args.message {
        message.clone().into_bytes()
    } else if let Some(file_path) = &args.file {
        debug!("Reading initial message from file: {:?}", file_path);
        fs::read(file_path).map_err(|e| anyhow!("Failed to read message file: {}", e))?
    } else {
        // Default initial message
        serde_json::to_vec(&serde_json::json!({
            "message_type": "channel_init",
            "payload": {
                "timestamp": chrono::Utc::now().timestamp_millis(),
            }
        }))
        .map_err(|e| anyhow!("Failed to create default initial message: {}", e))?
    };

    debug!("Initial message size: {} bytes", initial_message.len());
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = match TheaterId::parse(&args.actor_id) {
        Ok(id) => id,
        Err(e) => return Err(anyhow!("Invalid actor ID: {}", e)),
    };

    // Create tokio runtime
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        // Set up the interactive channel session
        run_channel_session(args.address, actor_id, initial_message, json, verbose).await
    })
}

async fn run_channel_session(
    server_addr: SocketAddr,
    actor_id: TheaterId,
    initial_message: Vec<u8>,
    json_output: bool,
    verbose: bool,
) -> Result<()> {
    let mut client = TheaterClient::new(server_addr);

    // Connect to the server
    client.connect().await?;

    println!(
        "{} Opening channel to actor: {}",
        style(">").green().bold(),
        style(actor_id.to_string()).cyan()
    );

    // Open a channel to the actor
    let channel_id = client
        .open_channel(actor_id.clone(), initial_message)
        .await?;

    println!(
        "{} Channel opened: {}",
        style("✓").green().bold(),
        style(&channel_id).cyan()
    );

    // Set up the REPL
    let mut rl = DefaultEditor::new()?;
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

    let mut running = true;

    // Main event loop using select!
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
                                    println!(
                                        "{} Read {} bytes from file",
                                        style("✓").green().bold(),
                                        content.len()
                                    );
                                    content
                                }
                                Err(e) => {
                                    println!("Error reading file: {}", e);
                                    continue;
                                }
                            }
                        } else {
                            println!("{} Sending message...", style(">").green().bold());
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
                        println!("{} Sending message...", style(">").green().bold());

                        // Send the message without competing for locks
                        match client.send_on_channel(&channel_id, message).await {
                            Ok(_) => {
                                if verbose {
                                    println!("{} Message sent", style("✓").green().bold());
                                }
                            }
                            Err(e) => {
                                println!(
                                    "{} Error sending message: {}",
                                    style("✗").red().bold(),
                                    e
                                );
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
                        println!(
                            "Unknown command: {}. Type 'help' for available commands.",
                            cmd
                        );
                    }
                }
            },

            // Check for incoming messages
            // This branch will run when there's no user input to process
            else => {
                match client.receive_channel_message().await {
                    Ok(response) => {
                        if let Some((id, message)) = response {
                            // Only process messages for our channel
                            if id == channel_id {
                                // Try to pretty print if it looks like JSON
                                match std::str::from_utf8(&message) {
                                    Ok(text) => {
                                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                                            if json_output {
                                                println!("{}", serde_json::to_string_pretty(&json).unwrap_or_else(|_| text.to_string()));
                                            } else {
                                                println!("\r{}", text);
                                            }
                                        } else {
                                            println!("\r{}", text);
                                        }
                                    },
                                    Err(_) => {
                                        println!("\r[Binary message of {} bytes]", message.len());
                                        if verbose {
                                            println!("\r{:?}", message);
                                        }
                                    }
                                }
                                // Re-display the prompt
                                print!("channel> ");
                                let _ = std::io::Write::flush(&mut std::io::stdout());
                            }
                        } else {
                            // No message received, small delay to prevent CPU spinning
                            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
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
    client.close_channel(&channel_id).await?;

    println!("{} Channel closed", style("✓").green().bold());

    Ok(())
}
