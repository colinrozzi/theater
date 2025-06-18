use std::time::Duration;
use tokio::sync::oneshot::{Receiver, Sender};
use tracing::debug;

/// Default timeout for waiting for a component to shutdown gracefully
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// A signal indicating that a component should shutdown
#[derive(Debug)]
pub struct ShutdownSignal {
    /// Type of shutdown to perform
    pub shutdown_type: ShutdownType,
    pub sender: Option<Sender<()>>,
}

/// Type of shutdown to perform
#[derive(Debug, Clone, Copy)]
pub enum ShutdownType {
    Graceful,
    Force,
}

/// Controller that can broadcast shutdown signals to multiple receivers
pub struct ShutdownController {
    subscribers: Vec<Sender<ShutdownSignal>>,
}

impl ShutdownController {
    /// Create a new ShutdownController and a ShutdownReceiver
    pub fn new() -> Self {
        Self {
            subscribers: Vec::new(),
        }
    }

    /// Get a new receiver for this controller
    pub fn subscribe(&mut self) -> ShutdownReceiver {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.subscribers.push(sender);
        ShutdownReceiver { receiver }
    }

    /// Signal all receivers to shutdown
    pub async fn signal_shutdown(self, shutdown_type: ShutdownType) {
        debug!("Signaling shutdown to all subscribers");
        let mut receivers = Vec::new();
        for sender in self.subscribers {
            let (responder, receiver) = tokio::sync::oneshot::channel();
            receivers.push(receiver);
            match sender.send(ShutdownSignal {
                shutdown_type,
                sender: Some(responder),
            }) {
                Ok(_) => {
                    debug!("Shutdown signal sent");
                }
                Err(e) => {
                    debug!("Failed to send shutdown signal: {:?}", e);
                }
            }
        }

        // Wait for all receivers to finish
        for receiver in receivers {
            if let Err(e) = receiver.await {
                debug!("Failed to receive shutdown signal: {:?}", e);
            }
        }
    }
}

/// Receiver that can wait for shutdown signals
pub struct ShutdownReceiver {
    pub receiver: Receiver<ShutdownSignal>,
}

impl ShutdownReceiver {
    /// Wait for a shutdown signal to be received
    pub async fn wait_for_shutdown(self) -> ShutdownSignal {
        debug!("Waiting for shutdown signal");
        match self.receiver.await {
            Ok(signal) => {
                debug!("Received shutdown signal");
                signal
            }
            Err(e) => {
                debug!("Shutdown channel error: {}, using default signal", e);
                ShutdownSignal {
                    sender: None,
                    shutdown_type: ShutdownType::Graceful,
                }
            }
        }
    }
}
