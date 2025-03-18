use anyhow::{anyhow, Result};
use clap::Parser;
use console::{style, Term};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

use theater::id::TheaterId;

use crate::cli::client::TheaterClient;
use crate::cli::utils::formatting;

#[derive(Debug, Parser)]
pub struct ShellArgs {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
    
    /// History size
    #[arg(long, default_value = "100")]
    pub history_size: usize,
}

// Command entered by the user
struct ShellCommand {
    full_line: String,
    command: String,
    args: Vec<String>,
}

pub fn execute(args: &ShellArgs, verbose: bool, _json: bool) -> Result<()> {
    debug!("Starting interactive shell");
    debug!("Connecting to server at: {}", args.address);
    
    // Setup terminal
    let term = Term::stdout();
    term.clear_screen()?;
    
    // Print welcome message
    println!("{}", formatting::format_section("THEATER INTERACTIVE SHELL"));
    println!("Connected to server at: {}", args.address);
    println!("Type 'help' for available commands or 'exit' to quit.");
    println!();
    
    // Create runtime
    let runtime = tokio::runtime::Runtime::new()?;
    
    // Initialize client and connect to server
    let client_result = runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);
        client.connect().await?;
        Ok::<TheaterClient, anyhow::Error>(client)
    });
    
    let mut client = match client_result {
        Ok(client) => client,
        Err(e) => {
            eprintln!("{}", formatting::format_error(&format!("Failed to connect to server: {}", e)));
            return Err(anyhow!("Failed to connect to server: {}", e));
        }
    };
    
    // Setup command history
    let mut history = VecDeque::with_capacity(args.history_size);
    let mut history_index = 0;
    
    // Setup running flag for ctrl+c handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    // Set up ctrl+c handler
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;
    
    // Main loop
    while running.load(Ordering::SeqCst) {
        // Display prompt
        print!("{} ", style("theater>").green().bold());
        io::stdout().flush()?;
        
        // Read input
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {},
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                continue;
            }
        }
        
        // Process input
        let input = input.trim();
        
        // Skip empty lines
        if input.is_empty() {
            continue;
        }
        
        // Add to history
        if !input.is_empty() && (history.is_empty() || history.back().map_or(true, |last| last != input)) {
            if history.len() == args.history_size {
                history.pop_front();
            }
            history.push_back(input.to_string());
        }
        history_index = history.len();
        
        // Parse command
        let command = parse_command(input);
        
        // Process command
        match command.command.as_str() {
            "exit" | "quit" => {
                println!("Goodbye!");
                break;
            },
            "help" => {
                print_help();
            },
            "list" => {
                // List actors
                handle_list_command(&mut client, &runtime)?;
            },
            "inspect" => {
                // Inspect an actor
                if command.args.is_empty() {
                    println!("{}", formatting::format_error("Missing actor ID"));
                    println!("Usage: inspect <actor-id>");
                } else {
                    handle_inspect_command(&mut client, &runtime, &command.args[0])?;
                }
            },
            "state" => {
                // Show actor state
                if command.args.is_empty() {
                    println!("{}", formatting::format_error("Missing actor ID"));
                    println!("Usage: state <actor-id>");
                } else {
                    handle_state_command(&mut client, &runtime, &command.args[0])?;
                }
            },
            "events" => {
                // Show actor events
                if command.args.is_empty() {
                    println!("{}", formatting::format_error("Missing actor ID"));
                    println!("Usage: events <actor-id>");
                } else {
                    handle_events_command(&mut client, &runtime, &command.args[0])?;
                }
            },
            "start" => {
                // Start an actor
                if command.args.is_empty() {
                    println!("{}", formatting::format_error("Missing manifest path"));
                    println!("Usage: start <manifest-path>");
                } else {
                    handle_start_command(&mut client, &runtime, &command.args[0])?;
                }
            },
            "stop" => {
                // Stop an actor
                if command.args.is_empty() {
                    println!("{}", formatting::format_error("Missing actor ID"));
                    println!("Usage: stop <actor-id>");
                } else {
                    handle_stop_command(&mut client, &runtime, &command.args[0])?;
                }
            },
            "restart" => {
                // Restart an actor
                if command.args.is_empty() {
                    println!("{}", formatting::format_error("Missing actor ID"));
                    println!("Usage: restart <actor-id>");
                } else {
                    handle_restart_command(&mut client, &runtime, &command.args[0])?;
                }
            },
            "message" => {
                // Send a message to an actor
                if command.args.len() < 2 {
                    println!("{}", formatting::format_error("Missing arguments"));
                    println!("Usage: message <actor-id> <message>");
                } else {
                    let message = command.args[1..].join(" ");
                    handle_message_command(&mut client, &runtime, &command.args[0], &message)?;
                }
            },
            "clear" => {
                // Clear the screen
                term.clear_screen()?;
            },
            _ => {
                println!("{}", formatting::format_error(&format!("Unknown command: {}", command.command)));
                println!("Type 'help' for available commands.");
            }
        }
    }
    
    Ok(())
}

/// Parse a command line into a ShellCommand
fn parse_command(line: &str) -> ShellCommand {
    let mut parts = line.split_whitespace();
    
    let command = parts.next().unwrap_or("").to_lowercase();
    let args: Vec<String> = parts.map(|s| s.to_string()).collect();
    
    ShellCommand {
        full_line: line.to_string(),
        command,
        args,
    }
}

/// Print help information
fn print_help() {
    println!("{}", formatting::format_section("AVAILABLE COMMANDS"));
    println!("{:<15} - {}", "list", "List all running actors");
    println!("{:<15} - {}", "inspect <id>", "Show detailed information about an actor");
    println!("{:<15} - {}", "state <id>", "Show the current state of an actor");
    println!("{:<15} - {}", "events <id>", "Show events for an actor");
    println!("{:<15} - {}", "start <path>", "Start an actor from a manifest");
    println!("{:<15} - {}", "stop <id>", "Stop a running actor");
    println!("{:<15} - {}", "restart <id>", "Restart a running actor");
    println!("{:<15} - {}", "message <id> <msg>", "Send a message to an actor");
    println!("{:<15} - {}", "clear", "Clear the screen");
    println!("{:<15} - {}", "help", "Show this help message");
    println!("{:<15} - {}", "exit", "Exit the shell");
}

/// Handle the 'list' command
fn handle_list_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime) -> Result<()> {
    let actors = runtime.block_on(async {
        client.list_actors().await
    })?;
    
    if actors.is_empty() {
        println!("No actors are currently running.");
        return Ok(());
    }
    
    println!("{}", formatting::format_section("RUNNING ACTORS"));
    for (i, actor_id) in actors.iter().enumerate() {
        // Get status
        let status = runtime.block_on(async {
            match client.get_actor_status(actor_id.clone()).await {
                Ok(status) => status,
                Err(_) => theater::messages::ActorStatus::Unknown,
            }
        });
        
        println!("{}. {} ({})", 
            i + 1, 
            formatting::format_id(actor_id),
            formatting::format_status(&status)
        );
    }
    
    Ok(())
}

/// Handle the 'inspect' command
fn handle_inspect_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime, id_str: &str) -> Result<()> {
    // Parse actor ID
    let actor_id = match id_str.parse::<TheaterId>() {
        Ok(id) => id,
        Err(_) => return Err(anyhow!("Invalid actor ID format: {}", id_str)),
    };
    
    // Get actor status
    let status = runtime.block_on(async {
        client.get_actor_status(actor_id.clone()).await
    })?;
    
    // Get actor state
    let state = runtime.block_on(async {
        match client.get_actor_state(actor_id.clone()).await {
            Ok(Some(state)) => {
                // Try to parse as JSON
                match serde_json::from_slice::<serde_json::Value>(&state) {
                    Ok(json) => Some(json),
                    Err(_) => None,
                }
            },
            _ => None,
        }
    });
    
    // Get actor events
    let events = runtime.block_on(async {
        match client.get_actor_events(actor_id.clone()).await {
            Ok(events) => events,
            Err(_) => vec![],
        }
    });
    
    // Print actor information
    println!("{}", formatting::format_section("ACTOR INFORMATION"));
    println!("{}", formatting::format_key_value("ID", &formatting::format_id(&actor_id)));
    println!("{}", formatting::format_key_value("Status", &formatting::format_status(&status)));
    
    // Print state information
    println!("{}", formatting::format_section("STATE"));
    if let Some(state_json) = state {
        println!("{}", serde_json::to_string_pretty(&state_json)?);
    } else {
        println!("No state available or not in JSON format");
    }
    
    // Print events information
    println!("{}", formatting::format_section("EVENTS"));
    println!("Total events: {}", events.len());
    
    if !events.is_empty() {
        println!("\nLatest events:");
        // Show the last 5 events
        let start_idx = if events.len() > 5 { events.len() - 5 } else { 0 };
        
        for (i, event) in events.iter().enumerate().skip(start_idx) {
            println!("{}. {}", i + 1, formatting::format_event_summary(event));
        }
    }
    
    Ok(())
}

/// Handle the 'state' command
fn handle_state_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime, id_str: &str) -> Result<()> {
    // Parse actor ID
    let actor_id = match id_str.parse::<TheaterId>() {
        Ok(id) => id,
        Err(_) => return Err(anyhow!("Invalid actor ID format: {}", id_str)),
    };
    
    // Get actor state
    let state = runtime.block_on(async {
        client.get_actor_state(actor_id.clone()).await
    })?;
    
    // Print state information
    println!("{}", formatting::format_section("ACTOR STATE"));
    println!("{}", formatting::format_key_value("ID", &formatting::format_id(&actor_id)));
    
    match state {
        Some(state_bytes) => {
            // Try to parse as JSON
            match serde_json::from_slice::<serde_json::Value>(&state_bytes) {
                Ok(json) => {
                    println!("\n{}", serde_json::to_string_pretty(&json)?);
                },
                Err(_) => {
                    println!("\n{} bytes of binary data", state_bytes.len());
                }
            }
        },
        None => {
            println!("\nNo state available");
        }
    }
    
    Ok(())
}

/// Handle the 'events' command
fn handle_events_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime, id_str: &str) -> Result<()> {
    // Parse actor ID
    let actor_id = match id_str.parse::<TheaterId>() {
        Ok(id) => id,
        Err(_) => return Err(anyhow!("Invalid actor ID format: {}", id_str)),
    };
    
    // Get actor events
    let events = runtime.block_on(async {
        client.get_actor_events(actor_id.clone()).await
    })?;
    
    // Print events information
    println!("{}", formatting::format_section("ACTOR EVENTS"));
    println!("{}", formatting::format_key_value("ID", &formatting::format_id(&actor_id)));
    println!("Total events: {}", events.len());
    
    if !events.is_empty() {
        println!();
        for (i, event) in events.iter().enumerate() {
            println!("{}. {}", i + 1, formatting::format_event_summary(event));
        }
    } else {
        println!("\nNo events available");
    }
    
    Ok(())
}

/// Handle the 'start' command
fn handle_start_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime, manifest_path: &str) -> Result<()> {
    // Check if manifest file exists
    let path = std::path::PathBuf::from(manifest_path);
    if !path.exists() {
        return Err(anyhow!("Manifest file not found: {}", manifest_path));
    }
    
    // Read manifest file
    let manifest_content = std::fs::read_to_string(&path)?;
    
    // Start the actor
    let actor_id = runtime.block_on(async {
        client.start_actor(manifest_content, None).await
    })?;
    
    println!("{}", formatting::format_success(&format!("Actor started: {}", formatting::format_id(&actor_id))));
    
    Ok(())
}

/// Handle the 'stop' command
fn handle_stop_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime, id_str: &str) -> Result<()> {
    // Parse actor ID
    let actor_id = match id_str.parse::<TheaterId>() {
        Ok(id) => id,
        Err(_) => return Err(anyhow!("Invalid actor ID format: {}", id_str)),
    };
    
    // Stop the actor
    runtime.block_on(async {
        client.stop_actor(actor_id.clone()).await
    })?;
    
    println!("{}", formatting::format_success(&format!("Actor stopped: {}", formatting::format_id(&actor_id))));
    
    Ok(())
}

/// Handle the 'restart' command
fn handle_restart_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime, id_str: &str) -> Result<()> {
    // Parse actor ID
    let actor_id = match id_str.parse::<TheaterId>() {
        Ok(id) => id,
        Err(_) => return Err(anyhow!("Invalid actor ID format: {}", id_str)),
    };
    
    // Restart the actor
    runtime.block_on(async {
        client.restart_actor(actor_id.clone()).await
    })?;
    
    println!("{}", formatting::format_success(&format!("Actor restarted: {}", formatting::format_id(&actor_id))));
    
    Ok(())
}

/// Handle the 'message' command
fn handle_message_command(client: &mut TheaterClient, runtime: &tokio::runtime::Runtime, id_str: &str, message: &str) -> Result<()> {
    // Parse actor ID
    let actor_id = match id_str.parse::<TheaterId>() {
        Ok(id) => id,
        Err(_) => return Err(anyhow!("Invalid actor ID format: {}", id_str)),
    };
    
    // Parse message as JSON
    let message_data = match serde_json::from_str::<serde_json::Value>(message) {
        Ok(json) => json.to_string().into_bytes(),
        Err(_) => {
            // Treat as plain string if not valid JSON
            message.as_bytes().to_vec()
        }
    };
    
    // Send the message
    runtime.block_on(async {
        client.send_actor_message(actor_id.clone(), message_data).await
    })?;
    
    println!("{}", formatting::format_success(&format!("Message sent to actor: {}", formatting::format_id(&actor_id))));
    
    Ok(())
}
