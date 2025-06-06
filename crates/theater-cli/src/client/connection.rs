use std::net::SocketAddr;
use anyhow::Result;
use bytes::Bytes;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, warn};

use theater_server::FragmentingCodec;

use crate::config::Config;
use crate::error::{CliError, CliResult};

pub use theater_server::{ManagementCommand, ManagementResponse};

/// A connection to the Theater server with automatic reconnection
#[derive(Debug)]
pub struct Connection {
    address: SocketAddr,
    config: Config,
    framed: Option<Framed<TcpStream, FragmentingCodec>>,
    last_error: Option<String>,
}

impl Connection {
    pub fn new(address: SocketAddr, config: Config) -> Self {
        Self {
            address,
            config,
            framed: None,
            last_error: None,
        }
    }

    /// Ensure we have an active connection, reconnecting if necessary
    pub async fn ensure_connected(&mut self) -> CliResult<()> {
        if self.framed.is_none() {
            self.connect().await?;
        }

        // Test the connection by sending a ping-like command
        // If it fails, try to reconnect once
        if !self.test_connection().await {
            info!("Connection test failed, attempting to reconnect");
            self.framed = None;
            self.connect().await?;
        }

        Ok(())
    }

    /// Establish a new connection to the server
    async fn connect(&mut self) -> CliResult<()> {
        info!("Connecting to Theater server at {}", self.address);

        let connect_future = TcpStream::connect(self.address);
        let socket = timeout(self.config.server.timeout, connect_future)
            .await
            .map_err(|_| CliError::ConnectionTimeout {
                timeout: self.config.server.timeout.as_secs(),
            })?
            .map_err(|e| CliError::connection_failed(self.address, e))?;

        // Use the FragmentingCodec for transparent message chunking
        let codec = FragmentingCodec::new();
        self.framed = Some(Framed::new(socket, codec));
        self.last_error = None;

        info!("Successfully connected to Theater server with fragmentation support");
        Ok(())
    }

    /// Test if the current connection is working
    async fn test_connection(&mut self) -> bool {
        if let Some(ref mut framed) = self.framed {
            // Try a simple ping by checking if we can write to the socket
            // In a real implementation, you'd send a proper ping command
            match framed.get_ref().peer_addr() {
                Ok(_) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Send a command and wait for a response
    pub async fn send_command(&mut self, command: ManagementCommand) -> CliResult<ManagementResponse> {
        self.ensure_connected().await?;

        let framed = self.framed.as_mut().unwrap();

        // Serialize and send the command
        debug!("Sending command: {:?}", command);
        let command_bytes = serde_json::to_vec(&command)
            .map_err(CliError::Serialization)?;

        // Send with timeout - FragmentingCodec will handle chunking if needed - FragmentingCodec will handle chunking if needed
        let send_future = framed.send(Bytes::from(command_bytes));
        timeout(self.config.server.timeout, send_future)
            .await
            .map_err(|_| CliError::ConnectionTimeout {
                timeout: self.config.server.timeout.as_secs(),
            })?
            .map_err(|e| {
                error!("Failed to send command: {}", e);
                CliError::ConnectionLost
            })?;

        debug!("Command sent, waiting for response");

        // Receive response with timeout - FragmentingCodec will handle reassembly if needed
        let receive_future = framed.next();
        let response_bytes = timeout(self.config.server.timeout, receive_future)
            .await
            .map_err(|_| CliError::ConnectionTimeout {
                timeout: self.config.server.timeout.as_secs(),
            })?;

        match response_bytes {
            Some(Ok(bytes)) => {
                let response: ManagementResponse = serde_json::from_slice(&bytes)
                    .map_err(|e| CliError::ProtocolError {
                        reason: format!("Failed to deserialize response: {}", e),
                    })?;
                debug!("Received response: {:?}", response);
                Ok(response)
            }
            Some(Err(e)) => {
                error!("Error receiving response: {}", e);
                self.framed = None; // Mark connection as broken
                Err(CliError::ConnectionLost)
            }
            None => {
                warn!("Connection closed by server");
                self.framed = None;
                Err(CliError::ConnectionLost)
            }
        }
    }

    /// Send a command without waiting for a response
    pub async fn send_command_no_response(&mut self, command: ManagementCommand) -> CliResult<()> {
        self.ensure_connected().await?;

        let framed = self.framed.as_mut().unwrap();

        // Serialize and send the command
        debug!("Sending command (no response expected): {:?}", command);
        let command_bytes = serde_json::to_vec(&command)
            .map_err(CliError::Serialization)?;

        // Send with timeout
        let send_future = framed.send(Bytes::from(command_bytes));
        timeout(self.config.server.timeout, send_future)
            .await
            .map_err(|_| CliError::ConnectionTimeout {
                timeout: self.config.server.timeout.as_secs(),
            })?
            .map_err(|e| {
                error!("Failed to send command: {}", e);
                CliError::ConnectionLost
            })?;

        debug!("Command sent (no response expected)");
        Ok(())
    }

    /// Get the next response from the connection (for streaming operations)
    pub async fn next_response(&mut self) -> CliResult<Option<ManagementResponse>> {
        if let Some(ref mut framed) = self.framed {
            let receive_future = framed.next();
            let response_bytes = timeout(self.config.server.timeout, receive_future)
                .await
                .map_err(|_| CliError::ConnectionTimeout {
                    timeout: self.config.server.timeout.as_secs(),
                })?;

            match response_bytes {
                Some(Ok(bytes)) => {
                    let response: ManagementResponse = serde_json::from_slice(&bytes)
                        .map_err(|e| CliError::ProtocolError {
                            reason: format!("Failed to deserialize response: {}", e),
                        })?;
                    debug!("Received streaming response: {:?}", response);
                    Ok(Some(response))
                }
                Some(Err(e)) => {
                    error!("Error receiving streaming response: {}", e);
                    self.framed = None;
                    Err(CliError::ConnectionLost)
                }
                None => {
                    debug!("Stream ended");
                    Ok(None)
                }
            }
        } else {
            Err(CliError::ConnectionLost)
        }
    }

    /// Check if the connection is currently active
    pub fn is_connected(&self) -> bool {
        self.framed.is_some()
    }

    /// Get the server address
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Get the last connection error, if any
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Close the connection
    pub async fn close(&mut self) {
        if let Some(mut framed) = self.framed.take() {
            let _ = framed.close().await;
            info!("Connection closed");
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if self.framed.is_some() {
            debug!("Connection dropped without explicit close");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_creation() {
        let config = Config::default();
        let addr = "127.0.0.1:9000".parse().unwrap();
        let conn = Connection::new(addr, config);
        
        assert_eq!(conn.address(), addr);
        assert!(!conn.is_connected());
        assert!(conn.last_error().is_none());
    }
}
