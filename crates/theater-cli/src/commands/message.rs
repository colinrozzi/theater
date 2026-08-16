//! `theater message` — poke a *running* actor from the command line.
//!
//! Unlike `spawn`/`setup` (which stand up a local runtime), these subcommands
//! connect to an already-running Theater runtime over its management TCP socket
//! and address a live actor by id. It's a debug/inspection tool — the same
//! `ManagementCommand`s an embedding client (e.g. a chat CLI via
//! `theater_client::TheaterConnection`) would send:
//!
//! - `send`      → `SendActorMessage`     (fire-and-forget; actor runs handle-send)
//! - `request`   → `RequestActorMessage`  (request/response; actor runs handle-request)
//! - `subscribe` → `SubscribeToActor`     (stream the actor's chain events)
//! - `channel`   → `OpenChannel`          (open a channel as an External participant,
//!                                          optionally post one message, watch replies)
//!
//! The target actor must run the message-server handler and implement the
//! matching `theater:simple/message-server-client` exports, or the runtime has
//! nothing to route these to.

use std::net::SocketAddr;

use clap::{Parser, Subcommand};
use tracing::debug;

use crate::{error::CliError, CommandContext};
use theater::id::TheaterId;
use theater::messages::ChannelParticipant;
use theater_client::{ManagementCommand, ManagementResponse, TheaterConnection};

#[derive(Debug, Parser)]
pub struct MessageArgs {
    /// Address of the running Theater server (defaults to the configured server address)
    #[arg(short, long, global = true)]
    pub address: Option<SocketAddr>,

    #[command(subcommand)]
    pub command: MessageCommand,
}

#[derive(Debug, Subcommand)]
pub enum MessageCommand {
    /// Fire-and-forget a message to a running actor (SendActorMessage)
    Send {
        /// Target actor id
        id: String,
        /// Message payload (sent as UTF-8 bytes)
        data: String,
    },
    /// Request/response RPC to a running actor (RequestActorMessage)
    Request {
        /// Target actor id
        id: String,
        /// Request payload (sent as UTF-8 bytes)
        data: String,
    },
    /// Subscribe to a running actor's chain events and stream them until Ctrl-C (SubscribeToActor)
    Subscribe {
        /// Target actor id
        id: String,
    },
    /// Open a channel to a running actor, optionally post one message, then watch replies (OpenChannel)
    Channel {
        /// Target actor id
        id: String,
        /// Optional initial message to post on open (sent as UTF-8 bytes)
        #[arg(short, long)]
        message: Option<String>,
    },
}

fn server_err(context: &str, e: impl std::fmt::Display) -> CliError {
    CliError::ServerError {
        message: format!("{context}: {e}"),
    }
}

fn parse_id(id: &str) -> Result<TheaterId, CliError> {
    id.parse::<TheaterId>()
        .map_err(|e| CliError::invalid_input("actor-id", id, &format!("not a valid actor id: {e}")))
}

/// Render response payload bytes for human eyes: UTF-8 if it is valid, else a
/// byte count (opaque binary payloads are common).
fn render_payload(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => format!("<{} bytes of binary>", bytes.len()),
    }
}

pub async fn execute_async(args: &MessageArgs, ctx: &CommandContext) -> Result<(), CliError> {
    let address = args.address.unwrap_or(ctx.config.server.default_address);
    debug!("Connecting to Theater server at {}", address);

    let mut conn = TheaterConnection::new(address);
    conn.connect()
        .await
        .map_err(|e| server_err(&format!("failed to connect to {address}"), e))?;

    match &args.command {
        MessageCommand::Send { id, data } => {
            let id = parse_id(id)?;
            let resp = conn
                .send_and_receive(ManagementCommand::SendActorMessage {
                    id,
                    data: data.clone().into_bytes(),
                })
                .await
                .map_err(|e| server_err("send failed", e))?;
            match resp {
                ManagementResponse::SentMessage { id } => {
                    ctx.output.success(&format!("message delivered to {id}"))?;
                }
                ManagementResponse::Error { error } => {
                    return Err(server_err("actor rejected message", format!("{error:?}")));
                }
                other => ctx.output.info(&format!("unexpected response: {other:?}"))?,
            }
        }

        MessageCommand::Request { id, data } => {
            let id = parse_id(id)?;
            let resp = conn
                .send_and_receive(ManagementCommand::RequestActorMessage {
                    id,
                    data: data.clone().into_bytes(),
                })
                .await
                .map_err(|e| server_err("request failed", e))?;
            match resp {
                ManagementResponse::RequestedMessage { id, message } => {
                    ctx.output.success(&format!("reply from {id}:"))?;
                    println!("{}", render_payload(&message));
                }
                ManagementResponse::Error { error } => {
                    return Err(server_err("request failed", format!("{error:?}")));
                }
                other => ctx.output.info(&format!("unexpected response: {other:?}"))?,
            }
        }

        MessageCommand::Subscribe { id } => {
            let id = parse_id(id)?;
            let resp = conn
                .send_and_receive(ManagementCommand::SubscribeToActor { id })
                .await
                .map_err(|e| server_err("subscribe failed", e))?;
            match resp {
                ManagementResponse::Subscribed { id, subscription_id } => {
                    ctx.output
                        .success(&format!("subscribed to {id} ({subscription_id}); Ctrl-C to stop"))?;
                }
                ManagementResponse::Error { error } => {
                    return Err(server_err("subscribe failed", format!("{error:?}")));
                }
                other => ctx.output.info(&format!("unexpected response: {other:?}"))?,
            }
            watch_loop(&mut conn, ctx).await?;
        }

        MessageCommand::Channel { id, message } => {
            let id = parse_id(id)?;
            let initial_message = message.clone().unwrap_or_default().into_bytes();
            let resp = conn
                .send_and_receive(ManagementCommand::OpenChannel {
                    actor_id: ChannelParticipant::Actor(id),
                    initial_message,
                })
                .await
                .map_err(|e| server_err("open channel failed", e))?;
            match resp {
                ManagementResponse::ChannelOpened { channel_id, actor_id } => {
                    ctx.output.success(&format!(
                        "channel {channel_id} open to {actor_id}; watching for messages, Ctrl-C to close"
                    ))?;
                }
                ManagementResponse::Error { error } => {
                    return Err(server_err("open channel failed", format!("{error:?}")));
                }
                other => ctx.output.info(&format!("unexpected response: {other:?}"))?,
            }
            watch_loop(&mut conn, ctx).await?;
        }
    }

    Ok(())
}

/// Stream responses (chain events / channel messages) until the connection
/// closes or the user cancels (Ctrl-C flips `shutdown_token`).
async fn watch_loop(conn: &mut TheaterConnection, ctx: &CommandContext) -> Result<(), CliError> {
    loop {
        tokio::select! {
            recv = conn.receive() => {
                match recv {
                    Ok(ManagementResponse::ActorEvent { event }) => {
                        println!("[event] {} ({} bytes)", event.event_type, event.data.len());
                    }
                    Ok(ManagementResponse::ChannelMessage { sender_id, message, .. }) => {
                        println!("[{sender_id}] {}", render_payload(&message));
                    }
                    Ok(ManagementResponse::ChannelClosed { channel_id }) => {
                        ctx.output.info(&format!("channel {channel_id} closed"))?;
                        break;
                    }
                    Ok(ManagementResponse::Error { error }) => {
                        return Err(server_err("stream error", format!("{error:?}")));
                    }
                    Ok(other) => {
                        debug!("ignoring response while watching: {other:?}");
                    }
                    Err(e) => {
                        // Connection closed by the server ends the watch cleanly.
                        ctx.output.info(&format!("stream ended: {e}"))?;
                        break;
                    }
                }
            }
            _ = ctx.shutdown_token.cancelled() => {
                break;
            }
        }
    }
    Ok(())
}
