//! WASI I/O Pollable resource implementation
//!
//! Provides pollable resources for streams and I/O events.

use std::sync::{Arc, Mutex, Weak};
use crate::streams::{InputStream, OutputStream, InputStreamState, OutputStreamState};

/// Pollable resource for I/O events
///
/// This represents an event that can be polled for readiness.
/// For streams, it tracks when the stream has data available or can accept writes.
#[derive(Debug, Clone)]
pub struct IoHandlerPollable {
    inner: PollableInner,
}

#[derive(Debug, Clone)]
enum PollableInner {
    /// Pollable for an input stream - ready when data is available to read
    InputStream(Weak<Mutex<InputStreamState>>),

    /// Pollable for an output stream - ready when writes can succeed
    OutputStream(Weak<Mutex<OutputStreamState>>),

    /// A pollable that is always ready (used for closed streams, etc.)
    Ready,

    /// A pollable that is never ready (used for blocked operations)
    Never,
}

impl IoHandlerPollable {
    /// Create a pollable for an input stream
    pub fn for_input_stream(stream: &InputStream) -> Self {
        // Get weak reference to the stream's internal state
        Self {
            inner: PollableInner::InputStream(Arc::downgrade(&stream.buffer_arc())),
        }
    }

    /// Create a pollable for an output stream
    pub fn for_output_stream(stream: &OutputStream) -> Self {
        // Get weak reference to the stream's internal state
        Self {
            inner: PollableInner::OutputStream(Arc::downgrade(&stream.buffer_arc())),
        }
    }

    /// Create a pollable that is always ready
    pub fn ready() -> Self {
        Self {
            inner: PollableInner::Ready,
        }
    }

    /// Create a pollable that is never ready
    pub fn never() -> Self {
        Self {
            inner: PollableInner::Never,
        }
    }

    /// Check if this pollable is ready (non-blocking)
    pub fn is_ready(&self) -> bool {
        match &self.inner {
            PollableInner::InputStream(weak) => {
                if let Some(arc) = weak.upgrade() {
                    let state = arc.lock().unwrap();
                    // Ready if there's data available or stream is closed
                    state.position < state.data.len() || state.closed
                } else {
                    // Stream was dropped, consider ready
                    true
                }
            }
            PollableInner::OutputStream(weak) => {
                if let Some(arc) = weak.upgrade() {
                    let state = arc.lock().unwrap();
                    // Ready if stream is not closed and not flushing
                    !state.closed && !state.flush_pending
                } else {
                    // Stream was dropped, consider ready
                    true
                }
            }
            PollableInner::Ready => true,
            PollableInner::Never => false,
        }
    }

    /// Block until this pollable is ready
    pub async fn block(&self) {
        // For in-memory streams, we just spin-check since they're always ready
        // In a real implementation, this would use proper async primitives
        while !self.is_ready() {
            tokio::task::yield_now().await;
        }
    }
}
