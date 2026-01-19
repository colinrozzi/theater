//! WASI HTTP outgoing-handler implementation
//!
//! Provides HTTP client capability using reqwest.

use crate::types::*;
use std::sync::{Arc, Mutex};

/// Future incoming response - represents a pending HTTP response
#[derive(Debug)]
pub struct FutureIncomingResponse {
    pub receiver: Arc<Mutex<Option<tokio::sync::oneshot::Receiver<Result<IncomingResponse, WasiErrorCode>>>>>,
}

impl FutureIncomingResponse {
    pub fn new(receiver: tokio::sync::oneshot::Receiver<Result<IncomingResponse, WasiErrorCode>>) -> Self {
        Self {
            receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }

    /// Subscribe to get a pollable for this future (for async waiting)
    pub fn subscribe(&self) -> u32 {
        // TODO: Return actual pollable resource
        0
    }

    /// Get the response (blocking operation)
    pub async fn get(self) -> Option<Result<IncomingResponse, WasiErrorCode>> {
        let mut guard = self.receiver.lock().unwrap();
        if let Some(receiver) = guard.take() {
            drop(guard); // Release lock before awaiting
            receiver.await.ok()
        } else {
            None
        }
    }
}
