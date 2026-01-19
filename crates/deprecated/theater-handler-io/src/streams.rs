//! WASI I/O Streams implementation
//!
//! Provides input-stream and output-stream resources for non-blocking I/O.

use crate::error::IoError;
use std::sync::{Arc, Mutex};
use tracing::{debug, trace};

/// Stream error type matching wasi:io/streams stream-error variant
#[derive(Debug, Clone)]
pub enum StreamError {
    /// The last operation failed
    LastOperationFailed(IoError),
    /// The stream is closed
    Closed,
}

impl From<IoError> for StreamError {
    fn from(err: IoError) -> Self {
        StreamError::LastOperationFailed(err)
    }
}

/// Input stream backed by an in-memory buffer
///
/// Provides non-blocking read operations on a byte stream.
#[derive(Debug, Clone)]
pub struct InputStream {
    /// The backing buffer
    buffer: Arc<Mutex<InputStreamState>>,
}

/// Internal state for an input stream (pub(crate) for pollable access)
#[derive(Debug)]
pub(crate) struct InputStreamState {
    /// The data buffer
    pub(crate) data: Vec<u8>,
    /// Current read position
    pub(crate) position: usize,
    /// Whether the stream is closed
    pub(crate) closed: bool,
}

impl InputStream {
    /// Create a new input stream from bytes
    pub fn from_bytes(data: Vec<u8>) -> Self {
        debug!("Creating input stream with {} bytes", data.len());
        Self {
            buffer: Arc::new(Mutex::new(InputStreamState {
                data,
                position: 0,
                closed: false,
            })),
        }
    }

    /// Create an empty closed stream
    pub fn closed() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(InputStreamState {
                data: Vec::new(),
                position: 0,
                closed: true,
            })),
        }
    }

    /// Read up to `len` bytes from the stream
    ///
    /// Returns immediately with available bytes (may be fewer than requested).
    /// Returns empty vec if no bytes are currently available.
    pub fn read(&self, len: u64) -> Result<Vec<u8>, StreamError> {
        let mut state = self.buffer.lock().unwrap();

        if state.closed && state.position >= state.data.len() {
            return Err(StreamError::Closed);
        }

        let available = state.data.len().saturating_sub(state.position);
        let to_read = (len as usize).min(available);

        if to_read == 0 {
            trace!("No bytes available to read");
            return Ok(Vec::new());
        }

        let end = state.position + to_read;
        let bytes = state.data[state.position..end].to_vec();
        state.position = end;

        trace!("Read {} bytes from input stream", bytes.len());
        Ok(bytes)
    }

    /// Skip up to `len` bytes in the stream
    ///
    /// Returns the number of bytes actually skipped.
    pub fn skip(&self, len: u64) -> Result<u64, StreamError> {
        let mut state = self.buffer.lock().unwrap();

        if state.closed && state.position >= state.data.len() {
            return Err(StreamError::Closed);
        }

        let available = state.data.len().saturating_sub(state.position);
        let to_skip = (len as usize).min(available);

        state.position += to_skip;

        trace!("Skipped {} bytes in input stream", to_skip);
        Ok(to_skip as u64)
    }

    /// Check if there are bytes available or if stream is closed
    pub fn is_ready(&self) -> bool {
        let state = self.buffer.lock().unwrap();
        state.position < state.data.len() || state.closed
    }

    /// Close the stream
    pub fn close(&self) {
        let mut state = self.buffer.lock().unwrap();
        state.closed = true;
    }

    /// Get the internal buffer Arc for pollable access
    pub(crate) fn buffer_arc(&self) -> Arc<Mutex<InputStreamState>> {
        Arc::clone(&self.buffer)
    }
}

/// Output stream backed by an in-memory buffer
///
/// Provides non-blocking write operations to a byte stream.
#[derive(Debug, Clone)]
pub struct OutputStream {
    /// The backing buffer
    buffer: Arc<Mutex<OutputStreamState>>,
}

/// Internal state for an output stream (pub(crate) for pollable access)
#[derive(Debug)]
pub(crate) struct OutputStreamState {
    /// The data buffer
    pub(crate) data: Vec<u8>,
    /// Whether the stream is closed
    pub(crate) closed: bool,
    /// Whether a flush is pending
    pub(crate) flush_pending: bool,
}

impl OutputStream {
    /// Create a new empty output stream
    pub fn new() -> Self {
        debug!("Creating new output stream");
        Self {
            buffer: Arc::new(Mutex::new(OutputStreamState {
                data: Vec::new(),
                closed: false,
                flush_pending: false,
            })),
        }
    }

    /// Check how many bytes can be written without blocking
    ///
    /// For in-memory streams, this is effectively unlimited (within memory constraints).
    /// Returns a large value to indicate capacity.
    pub fn check_write(&self) -> Result<u64, StreamError> {
        let state = self.buffer.lock().unwrap();

        if state.closed {
            return Err(StreamError::Closed);
        }

        if state.flush_pending {
            return Ok(0); // Can't write during flush
        }

        // Return a large value indicating we can accept writes
        // (in-memory buffer grows as needed)
        Ok(u64::MAX / 2)
    }

    /// Write bytes to the stream
    ///
    /// The caller should call check_write first to ensure capacity.
    pub fn write(&self, contents: &[u8]) -> Result<(), StreamError> {
        let mut state = self.buffer.lock().unwrap();

        if state.closed {
            return Err(StreamError::Closed);
        }

        if state.flush_pending {
            return Err(StreamError::LastOperationFailed(
                IoError::new("Cannot write during flush")
            ));
        }

        state.data.extend_from_slice(contents);
        trace!("Wrote {} bytes to output stream", contents.len());

        Ok(())
    }

    /// Request a flush of buffered data
    ///
    /// For in-memory streams, this is a no-op but sets the flush_pending flag.
    pub fn flush(&self) -> Result<(), StreamError> {
        let mut state = self.buffer.lock().unwrap();

        if state.closed {
            return Err(StreamError::Closed);
        }

        state.flush_pending = true;
        trace!("Flush requested on output stream");

        Ok(())
    }

    /// Complete the flush operation
    pub fn finish_flush(&self) {
        let mut state = self.buffer.lock().unwrap();
        state.flush_pending = false;
        trace!("Flush completed on output stream");
    }

    /// Get the current contents of the stream
    pub fn get_contents(&self) -> Vec<u8> {
        let state = self.buffer.lock().unwrap();
        state.data.clone()
    }

    /// Close the stream
    pub fn close(&self) {
        let mut state = self.buffer.lock().unwrap();
        state.closed = true;
    }

    /// Check if write operations would succeed (not closed, not flushing)
    pub fn is_ready(&self) -> bool {
        let state = self.buffer.lock().unwrap();
        !state.closed && !state.flush_pending
    }

    /// Get the internal buffer Arc for pollable access
    pub(crate) fn buffer_arc(&self) -> Arc<Mutex<OutputStreamState>> {
        Arc::clone(&self.buffer)
    }

    /// Write zeros to the stream
    ///
    /// More efficient than writing a buffer of zeros.
    pub fn write_zeroes(&self, len: u64) -> Result<(), StreamError> {
        let mut state = self.buffer.lock().unwrap();

        if state.closed {
            return Err(StreamError::Closed);
        }

        if state.flush_pending {
            return Err(StreamError::LastOperationFailed(
                IoError::new("Cannot write during flush")
            ));
        }

        // Extend with zeros
        let new_len = state.data.len() + len as usize;
        state.data.resize(new_len, 0);
        trace!("Wrote {} zero bytes to output stream", len);

        Ok(())
    }

    /// Splice bytes from an input stream to this output stream
    ///
    /// Copies up to `len` bytes from the input stream to this output stream.
    /// Returns the number of bytes actually copied.
    pub fn splice(&self, input: &InputStream, len: u64) -> Result<u64, StreamError> {
        // First, read from the input stream
        let data = input.read(len)?;
        let bytes_read = data.len() as u64;

        if bytes_read == 0 {
            return Ok(0);
        }

        // Then write to this output stream
        self.write(&data)?;

        trace!("Spliced {} bytes from input to output stream", bytes_read);
        Ok(bytes_read)
    }
}

impl Default for OutputStream {
    fn default() -> Self {
        Self::new()
    }
}

/// I/O handler placeholder for future expansion
pub struct IoHandler;
