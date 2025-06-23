use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};
use uuid::Uuid;

use theater::{messages::ChannelParticipant, ChainEvent};
use theater_server::{ManagementCommand, ManagementResponse};


use crate::error::{CliError, CliResult};
use theater_client::TheaterConnection;

/// High-level client for Theater server operations
#[derive(Debug, Clone)]
pub struct TheaterClient {
    connection: Arc<Mutex<TheaterConnection>>,
}

impl TheaterClient {
    /// Create a new TheaterClient
    pub fn new(address: SocketAddr) -> Self {
        let connection = TheaterConnection::new(address);
        Self {
            connection: Arc::new(Mutex::new(connection)),
        }
    }

    /// Get the server address
    pub async fn address(&self) -> SocketAddr {
        let conn = self.connection.lock().await;
        conn.address
    }

    /// Check if connected to the server
    pub async fn is_connected(&self) -> bool {
        let conn = self.connection.lock().await;
        conn.is_connected()
    }

    /// Explicitly connect to the server (usually not needed as commands auto-connect)
    pub async fn connect(&self) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        conn.connect()
            .await
            .map_err(|e| CliError::ConnectionFailed {
                address: conn.address,
                source: e,
            })
    }

    /// Close the connection
    pub async fn close(&self) {
        let mut conn = self.connection.lock().await;
        let _ = conn.close().await;
    }

    /// List all running actors
    pub async fn list_actors(&self) -> CliResult<Vec<(String, String)>> {
        let mut conn = self.connection.lock().await;
        let response = conn.send_and_receive(ManagementCommand::ListActors).await?;

        match response {
            ManagementResponse::ActorList { actors } => {
                debug!("Listed {} actors", actors.len());
                // Convert TheaterId to String for the CLI layer
                let string_actors: Vec<(String, String)> = actors
                    .into_iter()
                    .map(|(id, status)| (id.to_string(), status))
                    .collect();
                Ok(string_actors)
            }
            ManagementResponse::Error { error } => Err(CliError::ServerError {
                message: format!("{:?}", error),
            }),
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Start an actor from a manifest
    pub async fn start_actor(
        &self,
        manifest_content: String,
        initial_state: Option<Vec<u8>>,
        parent: bool,
        subscribe: bool,
    ) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        conn.send(ManagementCommand::StartActor {
            manifest: manifest_content,
            initial_state,
            parent,
            subscribe,
        })
        .await
        .map_err(|e| CliError::ConnectionFailed {
            address: conn.address,
            source: e,
        })
    }

    /// Stop a running actor
    pub async fn stop_actor(&self, actor_id: &str) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::StopActor { id: theater_id })
            .await?;

        match response {
            ManagementResponse::ActorStopped { id: _ } => {
                info!("Actor {} stopped successfully", actor_id);
                Ok(())
            }
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Get actor state
    pub async fn get_actor_state(&self, actor_id: &str) -> CliResult<serde_json::Value> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::GetActorState { id: theater_id })
            .await?;

        match response {
            ManagementResponse::ActorState { id: _, state } => {
                // Convert Vec<u8> state to JSON Value if present
                match state {
                    Some(bytes) => {
                        serde_json::from_slice(&bytes).map_err(|e| CliError::ParseError {
                            message: format!("Failed to parse actor state as JSON: {}", e),
                        })
                    }
                    None => Ok(serde_json::Value::Null),
                }
            }
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Get actor events
    pub async fn get_actor_events(&self, actor_id: &str) -> CliResult<Vec<ChainEvent>> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::GetActorEvents { id: theater_id })
            .await?;

        match response {
            ManagementResponse::ActorEvents { id: _, events } => {
                debug!("Retrieved {} events for actor {}", events.len(), actor_id);
                Ok(events)
            }
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Send a message to an actor (fire and forget)
    pub async fn send_message(&self, actor_id: &str, message: Vec<u8>) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::SendActorMessage {
                id: theater_id,
                data: message,
            })
            .await?;

        match response {
            ManagementResponse::SentMessage { id: _ } => Ok(()),
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Send a request to an actor and wait for response
    pub async fn request_message(&self, actor_id: &str, message: Vec<u8>) -> CliResult<Vec<u8>> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::RequestActorMessage {
                id: theater_id,
                data: message,
            })
            .await?;

        match response {
            ManagementResponse::RequestedMessage { id: _, message } => Ok(message),
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Subscribe to events from an actor (returns a stream-like interface)
    pub async fn subscribe_to_events(&self, actor_id: &str) -> CliResult<EventStream> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::SubscribeToActor { id: theater_id })
            .await?;

        match response {
            ManagementResponse::Subscribed {
                id: _,
                subscription_id,
            } => Ok(EventStream {
                client: self.clone(),
                actor_id: actor_id.to_string(),
                subscription_id,
            }),
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Get the next response from the connection (for streaming operations)
    pub async fn next_response(&self) -> CliResult<ManagementResponse> {
        let mut conn = self.connection.lock().await;
        conn.receive()
            .await
            .map_err(|e| CliError::ConnectionFailed {
                address: conn.address.clone(),
                source: e,
            })
    }

    /// Get actor status
    pub async fn get_actor_status(&self, actor_id: &str) -> CliResult<String> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::GetActorStatus { id: theater_id })
            .await?;

        match response {
            ManagementResponse::ActorStatus { id: _, status } => Ok(format!("{:?}", status)),
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Restart an actor
    pub async fn restart_actor(&self, actor_id: &str) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::RestartActor { id: theater_id })
            .await?;

        match response {
            ManagementResponse::Restarted { id: _ } => Ok(()),
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Update actor component
    pub async fn update_actor_component(&self, actor_id: &str, component: String) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::UpdateActorComponent {
                id: theater_id,
                component,
            })
            .await?;

        match response {
            ManagementResponse::ActorComponentUpdated { id: _ } => Ok(()),
            ManagementResponse::Error { error } => {
                let error_str = format!("{:?}", error);
                if error_str.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message: error_str })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Unsubscribe from actor events
    pub async fn unsubscribe_from_actor(
        &self,
        actor_id: &str,
        subscription_id: Uuid,
    ) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::UnsubscribeFromActor {
                id: theater_id,
                subscription_id,
            })
            .await?;

        match response {
            ManagementResponse::Unsubscribed { id: _ } => Ok(()),
            ManagementResponse::Error { error } => Err(CliError::ServerError {
                message: format!("{:?}", error),
            }),
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Open a channel with an actor
    pub async fn open_channel(
        &self,
        actor_id: &str,
        initial_message: Vec<u8>,
    ) -> CliResult<String> {
        let mut conn = self.connection.lock().await;
        let theater_id = actor_id
            .parse()
            .map_err(|_| CliError::invalid_actor_id(actor_id))?;
        let response = conn
            .send_and_receive(ManagementCommand::OpenChannel {
                actor_id: ChannelParticipant::Actor(theater_id),
                initial_message,
            })
            .await?;

        match response {
            ManagementResponse::ChannelOpened { channel_id, .. } => Ok(channel_id),
            ManagementResponse::Error { error } => Err(CliError::ServerError {
                message: format!("{:?}", error),
            }),
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Send a message on a channel
    pub async fn send_on_channel(&self, channel_id: &str, message: Vec<u8>) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_and_receive(ManagementCommand::SendOnChannel {
                channel_id: channel_id.to_string(),
                message,
            })
            .await?;

        match response {
            ManagementResponse::ChannelMessage { .. } => Ok(()),
            ManagementResponse::Error { error } => Err(CliError::ServerError {
                message: format!("{:?}", error),
            }),
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Close a channel
    pub async fn close_channel(&self, channel_id: &str) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_and_receive(ManagementCommand::CloseChannel {
                channel_id: channel_id.to_string(),
            })
            .await?;

        match response {
            ManagementResponse::ChannelClosed { .. } => Ok(()),
            ManagementResponse::Error { error } => Err(CliError::ServerError {
                message: format!("{:?}", error),
            }),
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Receive channel message (for channel communication)
    pub async fn receive_channel_message(&self) -> CliResult<Option<(String, Vec<u8>)>> {
        let mut conn = self.connection.lock().await;
        match conn.receive().await? {
            ManagementResponse::ChannelMessage {
                channel_id,
                message,
                ..
            } => Ok(Some((channel_id, message))),
            ManagementResponse::ChannelClosed { .. } => Ok(None),
            ManagementResponse::Error { error } => Err(CliError::ServerError {
                message: format!("{:?}", error),
            }),
            _ => {
                // Ignore other message types and try again
                Box::pin(self.receive_channel_message()).await
            }
        }
    }
}

/// A stream of events from an actor
pub struct EventStream {
    client: TheaterClient,
    actor_id: String,
    subscription_id: Uuid,
}

impl EventStream {
    /// Get the next event from the stream
    pub async fn next_event(&self) -> CliResult<Option<ChainEvent>> {
        let mut conn = self.client.connection.lock().await;
        match conn.receive().await? {
            ManagementResponse::ActorEvent { event } => Ok(Some(event)),
            ManagementResponse::ActorStopped { .. } => Ok(None),
            ManagementResponse::Error { error } => Err(CliError::EventStreamError {
                reason: format!("{:?}", error),
            }),
            _ => {
                // Ignore other response types in event stream
                Box::pin(self.next_event()).await
            }
        }
    }

    /// Get the actor ID this stream is associated with
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// Get the subscription ID
    pub fn subscription_id(&self) -> Uuid {
        self.subscription_id
    }

    /// Unsubscribe from this event stream
    pub async fn unsubscribe(self) -> CliResult<()> {
        self.client
            .unsubscribe_from_actor(&self.actor_id, self.subscription_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let addr = "127.0.0.1:9000".parse().unwrap();
        let client = TheaterClient::new(addr);

        assert_eq!(client.address().await, addr);
        assert!(!client.is_connected().await);
    }

    #[tokio::test]
    async fn test_client_clone() {
        let addr = "127.0.0.1:9000".parse().unwrap();
        let client = TheaterClient::new(addr);
        let client2 = client.clone();

        assert_eq!(client.address().await, client2.address().await);
    }
}
