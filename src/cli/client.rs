use anyhow::{anyhow, Result};
use bytes::Bytes;
use futures::sink::SinkExt;
use futures::stream::StreamExt;

use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, info};
use uuid::Uuid;

use theater::id::TheaterId;
use theater::messages::ActorStatus;
use theater::ChainEvent;

// Re-export the ManagementCommand and ManagementResponse types from the theater_server module
// This allows easier integration with the existing code
pub use theater::theater_server::{ManagementCommand, ManagementResponse};

/// A client for the Theater management API
pub struct TheaterClient {
    address: SocketAddr,
    connection: Option<Framed<TcpStream, LengthDelimitedCodec>>,
}

impl TheaterClient {
    /// Create a new TheaterClient
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            connection: None,
        }
    }

    /// Connect to the Theater server
    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting to Theater server at {}", self.address);
        let socket = TcpStream::connect(self.address).await?;
        let framed = Framed::new(socket, LengthDelimitedCodec::new());
        self.connection = Some(framed);
        info!("Connected to Theater server");
        Ok(())
    }

    /// Send a command to the Theater server and get the response
    pub async fn send_command(&mut self, command: ManagementCommand) -> Result<ManagementResponse> {
        // Make sure we have an active connection
        if self.connection.is_none() {
            self.connect().await?;
        }

        let connection = self.connection.as_mut().unwrap();
        
        // Serialize and send the command
        debug!("Sending command: {:?}", command);
        let command_bytes = serde_json::to_vec(&command)?;
        connection.send(Bytes::from(command_bytes)).await?;
        debug!("Command sent, awaiting response");

        // Receive and deserialize the response
        if let Some(response_bytes) = connection.next().await {
            let response_bytes = response_bytes?;
            let response: ManagementResponse = serde_json::from_slice(&response_bytes)?;
            debug!("Received response: {:?}", response);
            Ok(response)
        } else {
            // Connection closed
            self.connection = None;
            Err(anyhow!("Connection closed by server"))
        }
    }

    /// Start an actor from a manifest file with optional initial state
    pub async fn start_actor(&mut self, manifest: String, initial_state: Option<Vec<u8>>) -> Result<TheaterId> {
        let command = ManagementCommand::StartActor { manifest, initial_state };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ActorStarted { id } => Ok(id),
            ManagementResponse::Error { message } => Err(anyhow!("Error starting actor: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Stop a running actor
    pub async fn stop_actor(&mut self, id: TheaterId) -> Result<()> {
        let command = ManagementCommand::StopActor { id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ActorStopped { .. } => Ok(()),
            ManagementResponse::Error { message } => Err(anyhow!("Error stopping actor: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// List all running actors
    pub async fn list_actors(&mut self) -> Result<Vec<TheaterId>> {
        let command = ManagementCommand::ListActors;
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ActorList { actors } => Ok(actors),
            ManagementResponse::Error { message } => Err(anyhow!("Error listing actors: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Subscribe to events from an actor
    pub async fn subscribe_to_actor(&mut self, id: TheaterId) -> Result<Uuid> {
        let command = ManagementCommand::SubscribeToActor { id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::Subscribed { id: _, subscription_id } => Ok(subscription_id),
            ManagementResponse::Error { message } => Err(anyhow!("Error subscribing to actor: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Unsubscribe from events from an actor
    pub async fn unsubscribe_from_actor(&mut self, id: TheaterId, subscription_id: Uuid) -> Result<()> {
        let command = ManagementCommand::UnsubscribeFromActor { id, subscription_id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::Unsubscribed { .. } => Ok(()),
            ManagementResponse::Error { message } => Err(anyhow!("Error unsubscribing from actor: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Send a message to an actor
    pub async fn send_actor_message(&mut self, id: TheaterId, data: Vec<u8>) -> Result<()> {
        let command = ManagementCommand::SendActorMessage { id, data };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::SentMessage { .. } => Ok(()),
            ManagementResponse::Error { message } => Err(anyhow!("Error sending message to actor: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Request a message from an actor
    pub async fn request_actor_message(&mut self, id: TheaterId, data: Vec<u8>) -> Result<Vec<u8>> {
        let command = ManagementCommand::RequestActorMessage { id, data };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::RequestedMessage { message, .. } => Ok(message),
            ManagementResponse::Error { message } => Err(anyhow!("Error requesting message from actor: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Get the status of an actor
    pub async fn get_actor_status(&mut self, id: TheaterId) -> Result<ActorStatus> {
        let command = ManagementCommand::GetActorStatus { id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ActorStatus { status, .. } => Ok(status),
            ManagementResponse::Error { message } => Err(anyhow!("Error getting actor status: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Restart an actor
    pub async fn restart_actor(&mut self, id: TheaterId) -> Result<()> {
        let command = ManagementCommand::RestartActor { id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::Restarted { .. } => Ok(()),
            ManagementResponse::Error { message } => Err(anyhow!("Error restarting actor: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Get the state of an actor
    pub async fn get_actor_state(&mut self, id: TheaterId) -> Result<Option<Vec<u8>>> {
        let command = ManagementCommand::GetActorState { id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ActorState { state, .. } => Ok(state),
            ManagementResponse::Error { message } => Err(anyhow!("Error getting actor state: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Get the events of an actor
    pub async fn get_actor_events(&mut self, id: TheaterId) -> Result<Vec<ChainEvent>> {
        let command = ManagementCommand::GetActorEvents { id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ActorEvents { events, .. } => Ok(events),
            ManagementResponse::Error { message } => Err(anyhow!("Error getting actor events: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Get the metrics of an actor
    pub async fn get_actor_metrics(&mut self, id: TheaterId) -> Result<serde_json::Value> {
        let command = ManagementCommand::GetActorMetrics { id };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ActorMetrics { metrics, .. } => Ok(metrics),
            ManagementResponse::Error { message } => Err(anyhow!("Error getting actor metrics: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Open a channel to an actor
    pub async fn open_channel(&mut self, id: TheaterId, initial_message: Vec<u8>) -> Result<String> {
        let command = ManagementCommand::OpenChannel { 
            actor_id: id.clone(),
            initial_message 
        };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ChannelOpened { channel_id, .. } => Ok(channel_id),
            ManagementResponse::Error { message } => Err(anyhow!("Error opening channel: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Send a message on an existing channel
    pub async fn send_on_channel(&mut self, channel_id: &str, message: Vec<u8>) -> Result<()> {
        let command = ManagementCommand::SendOnChannel { 
            channel_id: channel_id.to_string(),
            message 
        };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::MessageSent { .. } => Ok(()),
            ManagementResponse::Error { message } => Err(anyhow!("Error sending on channel: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    /// Close an existing channel
    pub async fn close_channel(&mut self, channel_id: &str) -> Result<()> {
        let command = ManagementCommand::CloseChannel { 
            channel_id: channel_id.to_string() 
        };
        let response = self.send_command(command).await?;

        match response {
            ManagementResponse::ChannelClosed { .. } => Ok(()),
            ManagementResponse::Error { message } => Err(anyhow!("Error closing channel: {}", message)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }
    
    /// Receive a message on a channel (non-blocking)
    pub async fn receive_channel_message(&mut self) -> Result<Option<(String, Vec<u8>)>> {
        // Try to receive a response without sending a command first
        match self.receive_response().await {
            Ok(response) => {
                match response {
                    ManagementResponse::ChannelMessage { channel_id, message } => {
                        Ok(Some((channel_id, message)))
                    },
                    // Other responses are ignored as they're not relevant to our channel
                    _ => Ok(None)
                }
            },
            Err(_) => Ok(None) // No message available or other error
        }
    }
    
    /// Receive a response from the server without sending a command first
    /// Useful for receiving events from subscriptions
    pub async fn receive_response(&mut self) -> Result<ManagementResponse> {
        // Make sure we have an active connection
        if self.connection.is_none() {
            return Err(anyhow!("No active connection"));
        }

        let connection = self.connection.as_mut().unwrap();
        
        // Receive and deserialize the response
        if let Some(response_bytes) = connection.next().await {
            let response_bytes = response_bytes?;
            let response: ManagementResponse = serde_json::from_slice(&response_bytes)?;
            debug!("Received response: {:?}", response);
            Ok(response)
        } else {
            // Connection closed
            self.connection = None;
            Err(anyhow!("Connection closed by server"))
        }
    }
}
