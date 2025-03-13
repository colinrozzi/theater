use tokio::sync::broadcast;
use std::time::Duration;
use tracing::debug;

/// Default timeout for waiting for a component to shutdown gracefully
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// A signal indicating that a component should shutdown
#[derive(Debug, Clone)]
pub struct ShutdownSignal {}

/// Controller that can broadcast shutdown signals to multiple receivers
pub struct ShutdownController {
    sender: broadcast::Sender<ShutdownSignal>,
}

impl ShutdownController {
    /// Create a new ShutdownController and a ShutdownReceiver
    pub fn new() -> (Self, ShutdownReceiver) {
        let (sender, receiver) = broadcast::channel(8);
        (
            Self { sender },
            ShutdownReceiver { receiver },
        )
    }
    
    /// Get a new receiver for this controller
    pub fn subscribe(&self) -> ShutdownReceiver {
        ShutdownReceiver {
            receiver: self.sender.subscribe(),
        }
    }
    
    /// Signal all receivers to shutdown
    pub fn signal_shutdown(&self) {
        debug!("Broadcasting shutdown signal to {} receivers", self.sender.receiver_count());
        let send_count = self.sender.send(ShutdownSignal {}).unwrap_or(0);
        debug!("Shutdown signal sent to {} receivers", send_count);
    }
}

/// Receiver that can wait for shutdown signals
pub struct ShutdownReceiver {
    receiver: broadcast::Receiver<ShutdownSignal>,
}

impl ShutdownReceiver {
    /// Wait for a shutdown signal to be received
    pub async fn wait_for_shutdown(&mut self) -> ShutdownSignal {
        debug!("Waiting for shutdown signal");
        match self.receiver.recv().await {
            Ok(signal) => {
                debug!("Received shutdown signal");
                signal
            },
            Err(e) => {
                debug!("Shutdown channel error: {}, using default signal", e);
                ShutdownSignal {} // Default if channel closed
            }
        }
    }
}

impl Clone for ShutdownController {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}
