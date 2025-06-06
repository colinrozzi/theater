// CLI wrapper around theater-client with timeout and retry logic

use std::net::SocketAddr;
use theater_client::TheaterConnection;
use theater_server::{ManagementCommand, ManagementResponse};

use crate::config::Config;
use crate::error::{CliError, CliResult};

/// CLI wrapper around theater-client with timeout and retry logic
pub struct CliTheaterClient {
    connection: TheaterConnection,
    config: Config,
    address: SocketAddr,
}

impl CliTheaterClient {
    /// Create a new CLI client
    pub fn new(address: SocketAddr, config: Config) -> Self {
        Self {
            connection: TheaterConnection::new(address),
            config,
            address,
        }
    }

    /// Send a command and receive a response with CLI timeout and error handling
    pub async fn send_command(&mut self, command: ManagementCommand) -> CliResult<ManagementResponse> {
        // Connect with timeout
        self.ensure_connected().await?;

        // Send command with timeout
        tokio::time::timeout(
            self.config.server.timeout,
            self.connection.send(command)
        )
        .await
        .map_err(|_| CliError::ConnectionTimeout {
            timeout: self.config.server.timeout.as_secs(),
        })?
        .map_err(|e| CliError::connection_failed(self.address, e))?;

        // Receive response with timeout
        let response = tokio::time::timeout(
            self.config.server.timeout,
            self.connection.receive()
        )
        .await
        .map_err(|_| CliError::ConnectionTimeout {
            timeout: self.config.server.timeout.as_secs(),
        })?
        .map_err(|e| CliError::connection_failed(self.address, e))?;

        Ok(response)
    }

    /// Send a command without waiting for response (fire and forget)
    pub async fn send_command_no_response(&mut self, command: ManagementCommand) -> CliResult<()> {
        // Connect with timeout
        self.ensure_connected().await?;

        // Send command with timeout
        tokio::time::timeout(
            self.config.server.timeout,
            self.connection.send(command)
        )
        .await
        .map_err(|_| CliError::ConnectionTimeout {
            timeout: self.config.server.timeout.as_secs(),
        })?
        .map_err(|e| CliError::connection_failed(self.address, e))?;

        Ok(())
    }

    /// Receive the next response (for streaming operations)
    pub async fn receive(&mut self) -> CliResult<ManagementResponse> {
        let response = tokio::time::timeout(
            self.config.server.timeout,
            self.connection.receive()
        )
        .await
        .map_err(|_| CliError::ConnectionTimeout {
            timeout: self.config.server.timeout.as_secs(),
        })?
        .map_err(|e| CliError::connection_failed(self.address, e))?;

        Ok(response)
    }

    /// Ensure we're connected with retry logic
    async fn ensure_connected(&mut self) -> CliResult<()> {
        if self.connection.is_connected() {
            return Ok(());
        }

        let mut attempts = 0;
        let max_attempts = self.config.server.retry_attempts;

        while attempts < max_attempts {
            match tokio::time::timeout(
                self.config.server.timeout,
                self.connection.connect()
            ).await {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(e)) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(CliError::connection_failed(self.address, e));
                    }
                    // Wait before retry
                    tokio::time::sleep(self.config.server.retry_delay).await;
                }
                Err(_) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(CliError::ConnectionTimeout {
                            timeout: self.config.server.timeout.as_secs(),
                        });
                    }
                    tokio::time::sleep(self.config.server.retry_delay).await;
                }
            }
        }

        Err(CliError::ConnectionTimeout {
            timeout: self.config.server.timeout.as_secs(),
        })
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    /// Get the server address
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}

// Helper functions for common operations

/// List all actors with CLI error handling
pub async fn list_actors(address: SocketAddr, config: &Config) -> CliResult<Vec<(theater::id::TheaterId, String)>> {
    let mut client = CliTheaterClient::new(address, config.clone());
    let response = client.send_command(ManagementCommand::ListActors).await?;

    match response {
        ManagementResponse::ActorList { actors } => Ok(actors),
        ManagementResponse::Error { error } => Err(CliError::ServerError {
            message: format!("{:?}", error),
        }),
        _ => Err(CliError::UnexpectedResponse {
            response: format!("{:?}", response),
        }),
    }
}

/// Stop an actor with CLI error handling
pub async fn stop_actor(address: SocketAddr, config: &Config, actor_id: &str) -> CliResult<()> {
    let mut client = CliTheaterClient::new(address, config.clone());
    let theater_id = actor_id
        .parse()
        .map_err(|_| CliError::invalid_actor_id(actor_id))?;
    
    let response = client.send_command(ManagementCommand::StopActor { id: theater_id }).await?;

    match response {
        ManagementResponse::ActorStopped { .. } => Ok(()),
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

/// Get actor state with CLI error handling
pub async fn get_actor_state(address: SocketAddr, config: &Config, actor_id: &str) -> CliResult<Option<Vec<u8>>> {
    let mut client = CliTheaterClient::new(address, config.clone());
    let theater_id = actor_id
        .parse()
        .map_err(|_| CliError::invalid_actor_id(actor_id))?;
    
    let response = client.send_command(ManagementCommand::GetActorState { id: theater_id }).await?;

    match response {
        ManagementResponse::ActorState { state, .. } => Ok(state),
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
