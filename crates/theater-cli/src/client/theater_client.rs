use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

use theater::id::TheaterId;
use theater::messages::ActorStatus;
use theater::ChainEvent;

use crate::client::connection::Connection;
use crate::client::{ManagementCommand, ManagementResponse};
use crate::config::Config;
use crate::error::{CliError, CliResult};

/// High-level client for Theater server operations
#[derive(Debug, Clone)]
pub struct TheaterClient {
    connection: Arc<Mutex<Connection>>,
}

impl TheaterClient {
    /// Create a new TheaterClient
    pub fn new(address: SocketAddr, config: Config) -> Self {
        let connection = Connection::new(address, config);
        Self {
            connection: Arc::new(Mutex::new(connection)),
        }
    }

    /// Create a new TheaterClient with default configuration
    pub fn with_default_config(address: SocketAddr) -> Self {
        Self::new(address, Config::default())
    }

    /// Get the server address
    pub async fn address(&self) -> SocketAddr {
        let conn = self.connection.lock().await;
        conn.address()
    }

    /// Check if connected to the server
    pub async fn is_connected(&self) -> bool {
        let conn = self.connection.lock().await;
        conn.is_connected()
    }

    /// Explicitly connect to the server (usually not needed as commands auto-connect)
    pub async fn connect(&self) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        conn.ensure_connected().await
    }

    /// Close the connection
    pub async fn close(&self) {
        let mut conn = self.connection.lock().await;
        conn.close().await;
    }

    /// List all running actors
    pub async fn list_actors(&self) -> CliResult<Vec<(String, String)>> {
        let mut conn = self.connection.lock().await;
        let response = conn.send_command(ManagementCommand::ListActors).await?;

        match response {
            ManagementResponse::ActorList { actors } => {
                debug!("Listed {} actors", actors.len());
                Ok(actors)
            }
            ManagementResponse::Error { message } => {
                Err(CliError::ServerError { message })
            }
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
    ) -> CliResult<String> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_command(ManagementCommand::StartActor {
                manifest: manifest_content,
                initial_state,
                parent,
                subscribe,
            })
            .await?;

        match response {
            ManagementResponse::ActorStarted { id } => {
                info!("Actor started with ID: {}", id);
                Ok(id)
            }
            ManagementResponse::Error { message } => {
                Err(CliError::ActorStartFailed {
                    actor_id: "unknown".to_string(),
                    reason: message,
                })
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Stop a running actor
    pub async fn stop_actor(&self, actor_id: &str) -> CliResult<()> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_command(ManagementCommand::StopActor {
                id: actor_id.to_string(),
            })
            .await?;

        match response {
            ManagementResponse::ActorStopped { id: _ } => {
                info!("Actor {} stopped successfully", actor_id);
                Ok(())
            }
            ManagementResponse::Error { message } => {
                if message.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message })
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
        let response = conn
            .send_command(ManagementCommand::GetActorState {
                id: actor_id.to_string(),
            })
            .await?;

        match response {
            ManagementResponse::ActorState { state } => Ok(state),
            ManagementResponse::Error { message } => {
                if message.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Get actor events with filtering options
    pub async fn get_actor_events(
        &self,
        actor_id: &str,
        limit: Option<usize>,
        event_type: Option<String>,
        from: Option<String>,
        to: Option<String>,
        search: Option<String>,
    ) -> CliResult<Vec<ChainEvent>> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_command(ManagementCommand::GetActorEvents {
                id: actor_id.to_string(),
                limit,
                event_type,
                from,
                to,
                search,
            })
            .await?;

        match response {
            ManagementResponse::ActorEvents { events } => {
                debug!("Retrieved {} events for actor {}", events.len(), actor_id);
                Ok(events)
            }
            ManagementResponse::Error { message } => {
                if message.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message })
                }
            }
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Send a message to an actor
    pub async fn send_message(
        &self,
        actor_id: &str,
        message: serde_json::Value,
    ) -> CliResult<serde_json::Value> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_command(ManagementCommand::SendMessage {
                id: actor_id.to_string(),
                message,
            })
            .await?;

        match response {
            ManagementResponse::MessageResponse { response } => Ok(response),
            ManagementResponse::Error { message } => {
                if message.contains("not found") {
                    Err(CliError::actor_not_found(actor_id))
                } else {
                    Err(CliError::ServerError { message })
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
        conn.send_command_no_response(ManagementCommand::SubscribeToActor {
            id: actor_id.to_string(),
        })
        .await?;

        Ok(EventStream {
            client: self.clone(),
            actor_id: actor_id.to_string(),
        })
    }

    /// Get the next response from the connection (for streaming operations)
    pub async fn next_response(&self) -> CliResult<Option<ManagementResponse>> {
        let mut conn = self.connection.lock().await;
        conn.next_response().await
    }

    /// Validate a manifest file
    pub async fn validate_manifest(&self, manifest_content: &str) -> CliResult<bool> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_command(ManagementCommand::ValidateManifest {
                manifest: manifest_content.to_string(),
            })
            .await?;

        match response {
            ManagementResponse::ValidationResult { valid, .. } => Ok(valid),
            ManagementResponse::Error { message } => Err(CliError::ValidationError {
                reason: message,
            }),
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }

    /// Get server information
    pub async fn get_server_info(&self) -> CliResult<serde_json::Value> {
        let mut conn = self.connection.lock().await;
        let response = conn
            .send_command(ManagementCommand::GetServerInfo)
            .await?;

        match response {
            ManagementResponse::ServerInfo { info } => Ok(info),
            ManagementResponse::Error { message } => Err(CliError::ServerError { message }),
            _ => Err(CliError::UnexpectedResponse {
                response: format!("{:?}", response),
            }),
        }
    }
}

/// A stream of events from an actor
pub struct EventStream {
    client: TheaterClient,
    actor_id: String,
}

impl EventStream {
    /// Get the next event from the stream
    pub async fn next_event(&self) -> CliResult<Option<ChainEvent>> {
        match self.client.next_response().await? {
            Some(ManagementResponse::ActorEvent { event }) => Ok(Some(event)),
            Some(ManagementResponse::ActorStopped { .. }) => Ok(None),
            Some(ManagementResponse::Error { message }) => {
                Err(CliError::EventStreamError { reason: message })
            }
            Some(_) => {
                // Ignore other response types in event stream
                self.next_event().await
            }
            None => Ok(None),
        }
    }

    /// Get the actor ID this stream is associated with
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let addr = "127.0.0.1:9000".parse().unwrap();
        let client = TheaterClient::with_default_config(addr);
        
        assert_eq!(client.address().await, addr);
        assert!(!client.is_connected().await);
    }

    #[tokio::test]
    async fn test_client_clone() {
        let addr = "127.0.0.1:9000".parse().unwrap();
        let client = TheaterClient::with_default_config(addr);
        let client2 = client.clone();
        
        assert_eq!(client.address().await, client2.address().await);
    }
}
