//! # Actor Monitoring Example
//!
//! This example demonstrates how to use the TheaterConnection to
//! start an actor, monitor its lifecycle, and handle its completion.

use anyhow::Result;
use theater::client::TheaterConnection;
use theater::messages::{ActorResult, ActorStatus};
use theater::theater_server::{ManagementCommand, ManagementResponse};
use tokio::select;
use tokio::time::{interval, sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <manifest_path> [server_address]", args[0]);
        std::process::exit(1);
    }

    let manifest_path = &args[1];
    let server_address = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1:4000")
        .parse()?;

    println!("Connecting to server at {}", server_address);

    // Set up connection
    let mut conn = TheaterConnection::new(server_address);
    conn.connect().await?;

    println!("Starting actor from manifest: {}", manifest_path);

    // Start actor as parent (to receive completion notifications)
    conn.send(ManagementCommand::StartActor {
        manifest: manifest_path.to_string(),
        initial_state: None,
        parent: true,
        subscribe: true, // Subscribe to events
    })
    .await?;

    // Get the actor ID from the response
    let actor_id = loop {
        let response = conn.receive().await?;
        if let ManagementResponse::ActorStarted { id } = response {
            break id;
        }
    };

    println!("Actor started with ID: {}", actor_id);
    println!("Monitoring actor lifecycle...");

    // Set up a periodic status check (every 5 seconds)
    let mut heartbeat_interval = interval(Duration::from_secs(5));

    // Monitor the actor
    loop {
        select! {
            // Wait for the next response
            response = conn.receive() => {
                match response {
                    Ok(ManagementResponse::ActorEvent { event }) => {
                        println!("Event: {} - {:?}", event.event_type, event.timestamp);
                    },
                    Ok(ManagementResponse::ActorResult(result)) => {
                        match result {
                            ActorResult::Success(success) => {
                                println!("Actor completed successfully!");
                                if let Some(data) = success.result {
                                    println!("Result data: {}", String::from_utf8_lossy(&data));
                                }
                                break;
                            },
                            ActorResult::Error(error) => {
                                println!("Actor failed: {}", error.error);
                                break;
                            }
                        }
                    },
                    Ok(ManagementResponse::ActorError { error }) => {
                        println!("Actor error: {:?}", error);
                        break;
                    },
                    Ok(ManagementResponse::ActorStatus { status, .. }) => {
                        println!("Actor status: {:?}", status);

                        // If the actor is no longer running, exit
                        if !matches!(status, ActorStatus::Running) {
                            println!("Actor is no longer running: {:?}", status);
                            break;
                        }
                    },
                    Ok(other) => {
                        println!("Received: {:?}", other);
                    },
                    Err(e) => {
                        println!("Error: {}", e);
                        break;
                    }
                }
            },

            // Periodic heartbeat to check status
            _ = heartbeat_interval.tick() => {
                println!("Sending status check...");
                conn.send(ManagementCommand::GetActorStatus { id: actor_id.clone() }).await?;
            },

            // Optional timeout (1 hour)
            _ = sleep(Duration::from_secs(3600)) => {
                println!("Monitoring timeout reached (1 hour)");
                break;
            }
        }
    }

    println!("Actor monitoring complete");
    conn.close().await?;

    Ok(())
}
