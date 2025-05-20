//! # TCP Connection for Theater
//!
//! Provides a low-level TCP connection to a Theater server with simple
//! send and receive methods that can be used with tokio::select!

use anyhow::{anyhow, Result};
use bytes::Bytes;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error, info};

use crate::theater_server::{ManagementCommand, ManagementResponse};

/// A client connection to a Theater server
///
/// This provides a thin wrapper around the TCP connection with simple
/// send and receive methods that can be used with tokio::select!
pub struct TheaterConnection {
    /// Server address
    address: SocketAddr,
    /// The TCP connection
    connection: Option<Framed<TcpStream, LengthDelimitedCodec>>,
}

impl TheaterConnection {
    /// Create a new TheaterConnection
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the Theater server
    ///
    /// # Returns
    ///
    /// A new TheaterConnection instance (not yet connected)
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            connection: None,
        }
    }

    /// Connect to the Theater server
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the connection was successful
    /// * `Err(...)` if there was an error connecting
    pub async fn connect(&mut self) -> Result<()> {
        if self.connection.is_some() {
            return Ok(());
        }

        info!("Connecting to Theater server at {}", self.address);
        let socket = TcpStream::connect(self.address).await?;

        let mut codec = LengthDelimitedCodec::new();
        codec.set_max_frame_length(32 * 1024 * 1024); // 32MB max frame size
        let framed = Framed::new(socket, codec);

        self.connection = Some(framed);
        info!("Connected to Theater server");

        Ok(())
    }

    /// Send a command to the server
    ///
    /// # Arguments
    ///
    /// * `command` - The command to send
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the command was sent successfully
    /// * `Err(...)` if there was an error sending the command
    pub async fn send(&mut self, command: ManagementCommand) -> Result<()> {
        // Ensure we're connected
        if self.connection.is_none() {
            self.connect().await?;
        }

        // Serialize and send the command
        debug!("Sending command: {:?}", command);
        let command_bytes = serde_json::to_vec(&command)?;

        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| anyhow!("Connection lost"))?;

        connection.send(Bytes::from(command_bytes)).await?;
        debug!("Command sent");

        Ok(())
    }

    /// Receive a response from the server
    ///
    /// This method will wait for the next response from the server.
    /// It can be used with tokio::select! to handle multiple operations.
    ///
    /// # Returns
    ///
    /// * `Ok(response)` if a response was received
    /// * `Err(...)` if there was an error receiving the response
    pub async fn receive(&mut self) -> Result<ManagementResponse> {
        // Ensure we're connected
        if self.connection.is_none() {
            return Err(anyhow!("Not connected"));
        }

        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| anyhow!("Connection lost"))?;

        // Wait for the next message
        match connection.next().await {
            Some(Ok(bytes)) => {
                // Deserialize the response
                let response: ManagementResponse = serde_json::from_slice(&bytes)?;
                debug!("Received response: {:?}", response);
                Ok(response)
            }
            Some(Err(e)) => {
                error!("Error receiving response: {}", e);
                self.connection = None;
                Err(anyhow!("Connection error: {}", e))
            }
            None => {
                // Connection closed
                debug!("Connection closed by server");
                self.connection = None;
                Err(anyhow!("Connection closed by server"))
            }
        }
    }

    /// Check if the connection is active
    ///
    /// # Returns
    ///
    /// * `true` if the connection is active
    /// * `false` if the connection is not active
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    /// Close the connection
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the connection was closed successfully
    /// * `Err(...)` if there was an error closing the connection
    pub async fn close(&mut self) -> Result<()> {
        if let Some(mut connection) = self.connection.take() {
            connection.close().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    // Helper to run a mock server for testing
    async fn run_mock_server(addr: SocketAddr, shutdown_rx: oneshot::Receiver<()>) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        let shutdown_future = shutdown_rx;

        tokio::select! {
            _ = async {
                while let Ok((socket, _)) = listener.accept().await {
                    let mut framed = Framed::new(socket, LengthDelimitedCodec::new());

                    // Echo back whatever we receive
                    while let Some(Ok(bytes)) = framed.next().await {
                        framed.send(bytes.into()).await?;
                    }
                }
                Ok::<(), anyhow::Error>(())
            } => {},
            _ = shutdown_future => {},
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_connection() -> Result<()> {
        // Bind to a random port
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        drop(listener);

        // Start mock server
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(run_mock_server(addr, shutdown_rx));

        // Allow server to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create client and connect
        let mut client = TheaterConnection::new(addr);
        client.connect().await?;
        assert!(client.is_connected());

        // Clean up
        client.close().await?;
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;

        Ok(())
    }
}
