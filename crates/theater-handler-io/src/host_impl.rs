//! Host trait implementations for WASI I/O and CLI interfaces
//!
//! These implementations provide I/O capabilities to actors while
//! recording all operations in the event chain for replay.

use crate::bindings::{
    ErrorHost, HostError, PollHost, HostPollable,
    StreamsHost, HostInputStream, HostOutputStream,
    StdinHost, StdoutHost, StderrHost,
    EnvironmentHost, ExitHost,
    TerminalInputHost, HostTerminalInput,
    TerminalOutputHost, HostTerminalOutput,
    TerminalStdinHost, TerminalStdoutHost, TerminalStderrHost,
};
use crate::events::IoEventData;
use crate::poll::IoHandlerPollable;
use crate::{InputStream, OutputStream, IoError, CliArguments, InitialCwd};
use crate::streams::StreamError as InternalStreamError;
use anyhow::Result;
use wasmtime::component::Resource;
use theater::actor::ActorStore;
use theater::events::EventPayload;
use tracing::{debug, warn};

// =============================================================================
// wasi:io/error Host implementation
// =============================================================================

impl<E> ErrorHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    // No non-resource methods in the error interface
}

impl<E> HostError for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn to_debug_string(&mut self, error: Resource<IoError>) -> Result<String> {
        let error_id = error.rep();
        debug!("wasi:io/error to-debug-string: {}", error_id);

        let table = self.resource_table.lock().unwrap();
        let io_error: &IoError = table.get(&error)?;
        let debug_string = io_error.to_debug_string();

        Ok(debug_string)
    }

    async fn drop(&mut self, error: Resource<IoError>) -> Result<()> {
        let error_id = error.rep();
        debug!("wasi:io/error drop: {}", error_id);

        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(error);
        Ok(())
    }
}

// =============================================================================
// wasi:io/poll Host implementation
// =============================================================================

impl<E> PollHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn poll(&mut self, pollables: Vec<Resource<IoHandlerPollable>>) -> Result<Vec<u32>> {
        debug!("wasi:io/poll poll: {} pollables", pollables.len());

        self.record_handler_event(
            "wasi:io/poll/poll".to_string(),
            IoEventData::PollCall { num_pollables: pollables.len() },
            Some(format!("WASI poll: polling {} pollables", pollables.len())),
        );

        let mut ready_indices = Vec::new();

        // Check each pollable
        for (idx, pollable_handle) in pollables.iter().enumerate() {
            let is_ready = {
                let table = self.resource_table.lock().unwrap();
                if let Ok(pollable) = table.get(pollable_handle) {
                    pollable.is_ready()
                } else {
                    false
                }
            };

            if is_ready {
                ready_indices.push(idx as u32);
            }
        }

        self.record_handler_event(
            "wasi:io/poll/poll".to_string(),
            IoEventData::PollResult { ready_indices: ready_indices.clone() },
            Some(format!("WASI poll: {} pollables ready", ready_indices.len())),
        );

        Ok(ready_indices)
    }
}

impl<E> HostPollable for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn ready(&mut self, pollable_handle: Resource<IoHandlerPollable>) -> Result<bool> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.ready: {}", pollable_id);

        self.record_handler_event(
            "wasi:io/poll/pollable.ready".to_string(),
            IoEventData::PollableReadyCall { pollable_id },
            Some(format!("WASI poll: checking if pollable {} is ready", pollable_id)),
        );

        let is_ready = {
            let table = self.resource_table.lock().unwrap();
            if let Ok(pollable) = table.get(&pollable_handle) {
                pollable.is_ready()
            } else {
                false
            }
        };

        self.record_handler_event(
            "wasi:io/poll/pollable.ready".to_string(),
            IoEventData::PollableReadyResult { pollable_id, is_ready },
            Some(format!("WASI poll: pollable {} ready={}", pollable_id, is_ready)),
        );

        Ok(is_ready)
    }

    async fn block(&mut self, pollable_handle: Resource<IoHandlerPollable>) -> Result<()> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.block: {}", pollable_id);

        self.record_handler_event(
            "wasi:io/poll/pollable.block".to_string(),
            IoEventData::PollableBlockCall { pollable_id },
            Some(format!("WASI poll: blocking on pollable {}", pollable_id)),
        );

        // Get the pollable and block on it
        let pollable = {
            let table = self.resource_table.lock().unwrap();
            table.get(&pollable_handle)?.clone()
        };

        pollable.block().await;

        self.record_handler_event(
            "wasi:io/poll/pollable.block".to_string(),
            IoEventData::PollableBlockResult { pollable_id },
            Some(format!("WASI poll: pollable {} unblocked", pollable_id)),
        );

        Ok(())
    }

    async fn drop(&mut self, pollable_handle: Resource<IoHandlerPollable>) -> Result<()> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.drop: {}", pollable_id);

        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(pollable_handle);
        Ok(())
    }
}

// =============================================================================
// wasi:io/streams Host implementation
// =============================================================================

// Helper to convert internal StreamError to the bindings StreamError
fn to_bindings_stream_error(e: InternalStreamError) -> crate::bindings::wasi::io::streams::StreamError {
    match e {
        InternalStreamError::LastOperationFailed(_) => {
            crate::bindings::wasi::io::streams::StreamError::Closed
        }
        InternalStreamError::Closed => {
            crate::bindings::wasi::io::streams::StreamError::Closed
        }
    }
}

impl<E> StreamsHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    // No non-resource methods in streams interface
}

impl<E> HostInputStream for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn read(
        &mut self,
        stream: Resource<InputStream>,
        len: u64,
    ) -> Result<Result<Vec<u8>, crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams input-stream.read: len={}", len);

        self.record_handler_event(
            "wasi:io/streams/input-stream.read".to_string(),
            IoEventData::InputStreamReadCall { len },
            Some(format!("WASI I/O: reading up to {} bytes from input stream", len)),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let input_stream: &InputStream = table.get(&stream)?;
            input_stream.read(len)
        };

        match result {
            Ok(data) => {
                let bytes_read = data.len();
                self.record_handler_event(
                    "wasi:io/streams/input-stream.read".to_string(),
                    IoEventData::InputStreamReadResult { bytes_read, success: true },
                    Some(format!("WASI I/O: read {} bytes", bytes_read)),
                );
                Ok(Ok(data))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/input-stream.read".to_string(),
                    IoEventData::InputStreamReadResult { bytes_read: 0, success: false },
                    Some("WASI I/O: read error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn blocking_read(
        &mut self,
        stream: Resource<InputStream>,
        len: u64,
    ) -> Result<Result<Vec<u8>, crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams input-stream.blocking-read: len={}", len);

        self.record_handler_event(
            "wasi:io/streams/input-stream.blocking-read".to_string(),
            IoEventData::InputStreamReadCall { len },
            Some(format!("WASI I/O: blocking read up to {} bytes", len)),
        );

        // For in-memory streams, blocking-read is the same as read
        let result = {
            let table = self.resource_table.lock().unwrap();
            let input_stream: &InputStream = table.get(&stream)?;
            input_stream.read(len)
        };

        match result {
            Ok(data) => {
                let bytes_read = data.len();
                self.record_handler_event(
                    "wasi:io/streams/input-stream.blocking-read".to_string(),
                    IoEventData::InputStreamReadResult { bytes_read, success: true },
                    Some(format!("WASI I/O: blocking read {} bytes", bytes_read)),
                );
                Ok(Ok(data))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/input-stream.blocking-read".to_string(),
                    IoEventData::InputStreamReadResult { bytes_read: 0, success: false },
                    Some("WASI I/O: blocking read error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn skip(
        &mut self,
        stream: Resource<InputStream>,
        len: u64,
    ) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams input-stream.skip: len={}", len);

        self.record_handler_event(
            "wasi:io/streams/input-stream.skip".to_string(),
            IoEventData::InputStreamSkipCall { len },
            Some(format!("WASI I/O: skipping up to {} bytes", len)),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let input_stream: &InputStream = table.get(&stream)?;
            input_stream.skip(len)
        };

        match result {
            Ok(bytes_skipped) => {
                self.record_handler_event(
                    "wasi:io/streams/input-stream.skip".to_string(),
                    IoEventData::InputStreamSkipResult { bytes_skipped, success: true },
                    Some(format!("WASI I/O: skipped {} bytes", bytes_skipped)),
                );
                Ok(Ok(bytes_skipped))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/input-stream.skip".to_string(),
                    IoEventData::InputStreamSkipResult { bytes_skipped: 0, success: false },
                    Some("WASI I/O: skip error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn blocking_skip(
        &mut self,
        stream: Resource<InputStream>,
        len: u64,
    ) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams input-stream.blocking-skip: len={}", len);

        self.record_handler_event(
            "wasi:io/streams/input-stream.blocking-skip".to_string(),
            IoEventData::InputStreamSkipCall { len },
            Some(format!("WASI I/O: blocking skip up to {} bytes", len)),
        );

        // For in-memory streams, blocking-skip is the same as skip
        let result = {
            let table = self.resource_table.lock().unwrap();
            let input_stream: &InputStream = table.get(&stream)?;
            input_stream.skip(len)
        };

        match result {
            Ok(bytes_skipped) => {
                self.record_handler_event(
                    "wasi:io/streams/input-stream.blocking-skip".to_string(),
                    IoEventData::InputStreamSkipResult { bytes_skipped, success: true },
                    Some(format!("WASI I/O: blocking skipped {} bytes", bytes_skipped)),
                );
                Ok(Ok(bytes_skipped))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/input-stream.blocking-skip".to_string(),
                    IoEventData::InputStreamSkipResult { bytes_skipped: 0, success: false },
                    Some("WASI I/O: blocking skip error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn subscribe(&mut self, stream: Resource<InputStream>) -> Result<Resource<IoHandlerPollable>> {
        debug!("wasi:io/streams input-stream.subscribe");

        self.record_handler_event(
            "wasi:io/streams/input-stream.subscribe".to_string(),
            IoEventData::InputStreamSubscribeCall,
            Some("WASI I/O: subscribing to input stream".to_string()),
        );

        // Create a pollable for this stream
        let pollable = {
            let table = self.resource_table.lock().unwrap();
            let input_stream: &InputStream = table.get(&stream)?;
            IoHandlerPollable::for_input_stream(input_stream)
        };

        // Push the pollable to the resource table
        let pollable_handle = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(pollable)?
        };

        let pollable_id = pollable_handle.rep();
        self.record_handler_event(
            "wasi:io/streams/input-stream.subscribe".to_string(),
            IoEventData::InputStreamSubscribeResult { pollable_id },
            Some(format!("WASI I/O: created pollable {}", pollable_id)),
        );

        Ok(pollable_handle)
    }

    async fn drop(&mut self, stream: Resource<InputStream>) -> Result<()> {
        debug!("wasi:io/streams input-stream.drop");

        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(stream);
        Ok(())
    }
}

impl<E> HostOutputStream for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn check_write(
        &mut self,
        stream: Resource<OutputStream>,
    ) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.check-write");

        self.record_handler_event(
            "wasi:io/streams/output-stream.check-write".to_string(),
            IoEventData::OutputStreamCheckWriteCall,
            Some("WASI I/O: checking write capacity".to_string()),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            output_stream.check_write()
        };

        match result {
            Ok(available) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.check-write".to_string(),
                    IoEventData::OutputStreamCheckWriteResult { available, success: true },
                    Some(format!("WASI I/O: {} bytes available for writing", available)),
                );
                Ok(Ok(available))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.check-write".to_string(),
                    IoEventData::OutputStreamCheckWriteResult { available: 0, success: false },
                    Some("WASI I/O: check-write error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn write(
        &mut self,
        stream: Resource<OutputStream>,
        contents: Vec<u8>,
    ) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.write: {} bytes", contents.len());

        let len = contents.len();
        self.record_handler_event(
            "wasi:io/streams/output-stream.write".to_string(),
            IoEventData::OutputStreamWriteCall { len },
            Some(format!("WASI I/O: writing {} bytes", len)),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            output_stream.write(&contents)
        };

        match result {
            Ok(()) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.write".to_string(),
                    IoEventData::OutputStreamWriteResult { bytes_written: len, success: true },
                    Some(format!("WASI I/O: wrote {} bytes", len)),
                );
                Ok(Ok(()))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.write".to_string(),
                    IoEventData::OutputStreamWriteResult { bytes_written: 0, success: false },
                    Some("WASI I/O: write error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn blocking_write_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        contents: Vec<u8>,
    ) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.blocking-write-and-flush: {} bytes", contents.len());

        let len = contents.len();
        self.record_handler_event(
            "wasi:io/streams/output-stream.blocking-write-and-flush".to_string(),
            IoEventData::OutputStreamWriteCall { len },
            Some(format!("WASI I/O: blocking write and flush {} bytes", len)),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            output_stream.write(&contents).and_then(|_| output_stream.flush())
        };

        match result {
            Ok(()) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-write-and-flush".to_string(),
                    IoEventData::OutputStreamWriteResult { bytes_written: len, success: true },
                    Some(format!("WASI I/O: blocking wrote and flushed {} bytes", len)),
                );
                Ok(Ok(()))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-write-and-flush".to_string(),
                    IoEventData::OutputStreamWriteResult { bytes_written: 0, success: false },
                    Some("WASI I/O: blocking write and flush error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn flush(
        &mut self,
        stream: Resource<OutputStream>,
    ) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.flush");

        self.record_handler_event(
            "wasi:io/streams/output-stream.flush".to_string(),
            IoEventData::OutputStreamFlushCall,
            Some("WASI I/O: flushing output stream".to_string()),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            output_stream.flush()
        };

        match result {
            Ok(()) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.flush".to_string(),
                    IoEventData::OutputStreamFlushResult { success: true },
                    Some("WASI I/O: flush completed".to_string()),
                );
                Ok(Ok(()))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.flush".to_string(),
                    IoEventData::OutputStreamFlushResult { success: false },
                    Some("WASI I/O: flush error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn blocking_flush(
        &mut self,
        stream: Resource<OutputStream>,
    ) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.blocking-flush");

        self.record_handler_event(
            "wasi:io/streams/output-stream.blocking-flush".to_string(),
            IoEventData::OutputStreamFlushCall,
            Some("WASI I/O: blocking flush".to_string()),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            output_stream.flush()
        };

        match result {
            Ok(()) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-flush".to_string(),
                    IoEventData::OutputStreamFlushResult { success: true },
                    Some("WASI I/O: blocking flush completed".to_string()),
                );
                Ok(Ok(()))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-flush".to_string(),
                    IoEventData::OutputStreamFlushResult { success: false },
                    Some("WASI I/O: blocking flush error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn subscribe(&mut self, stream: Resource<OutputStream>) -> Result<Resource<IoHandlerPollable>> {
        debug!("wasi:io/streams output-stream.subscribe");

        self.record_handler_event(
            "wasi:io/streams/output-stream.subscribe".to_string(),
            IoEventData::OutputStreamSubscribeCall,
            Some("WASI I/O: subscribing to output stream".to_string()),
        );

        // Create a pollable for this stream
        let pollable = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            IoHandlerPollable::for_output_stream(output_stream)
        };

        // Push the pollable to the resource table
        let pollable_handle = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(pollable)?
        };

        let pollable_id = pollable_handle.rep();
        self.record_handler_event(
            "wasi:io/streams/output-stream.subscribe".to_string(),
            IoEventData::OutputStreamSubscribeResult { pollable_id },
            Some(format!("WASI I/O: created pollable {}", pollable_id)),
        );

        Ok(pollable_handle)
    }

    async fn write_zeroes(
        &mut self,
        stream: Resource<OutputStream>,
        len: u64,
    ) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.write-zeroes: {} bytes", len);

        self.record_handler_event(
            "wasi:io/streams/output-stream.write-zeroes".to_string(),
            IoEventData::OutputStreamWriteZeroesCall { len },
            Some(format!("WASI I/O: writing {} zero bytes", len)),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            output_stream.write_zeroes(len)
        };

        match result {
            Ok(()) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.write-zeroes".to_string(),
                    IoEventData::OutputStreamWriteZeroesResult { bytes_written: len, success: true },
                    Some(format!("WASI I/O: wrote {} zero bytes", len)),
                );
                Ok(Ok(()))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.write-zeroes".to_string(),
                    IoEventData::OutputStreamWriteZeroesResult { bytes_written: 0, success: false },
                    Some("WASI I/O: write-zeroes error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn blocking_write_zeroes_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        len: u64,
    ) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.blocking-write-zeroes-and-flush: {} bytes", len);

        self.record_handler_event(
            "wasi:io/streams/output-stream.blocking-write-zeroes-and-flush".to_string(),
            IoEventData::OutputStreamWriteZeroesCall { len },
            Some(format!("WASI I/O: blocking write {} zero bytes and flush", len)),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&stream)?;
            output_stream.write_zeroes(len).and_then(|_| output_stream.flush())
        };

        match result {
            Ok(()) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-write-zeroes-and-flush".to_string(),
                    IoEventData::OutputStreamWriteZeroesResult { bytes_written: len, success: true },
                    Some(format!("WASI I/O: blocking wrote {} zero bytes and flushed", len)),
                );
                Ok(Ok(()))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-write-zeroes-and-flush".to_string(),
                    IoEventData::OutputStreamWriteZeroesResult { bytes_written: 0, success: false },
                    Some("WASI I/O: blocking write-zeroes-and-flush error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn splice(
        &mut self,
        output: Resource<OutputStream>,
        input: Resource<InputStream>,
        len: u64,
    ) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.splice: len={}", len);

        self.record_handler_event(
            "wasi:io/streams/output-stream.splice".to_string(),
            IoEventData::OutputStreamSpliceCall { len },
            Some(format!("WASI I/O: splicing up to {} bytes", len)),
        );

        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&output)?;
            let input_stream: &InputStream = table.get(&input)?;
            output_stream.splice(input_stream, len)
        };

        match result {
            Ok(bytes_spliced) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.splice".to_string(),
                    IoEventData::OutputStreamSpliceResult { bytes_spliced, success: true },
                    Some(format!("WASI I/O: spliced {} bytes", bytes_spliced)),
                );
                Ok(Ok(bytes_spliced))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.splice".to_string(),
                    IoEventData::OutputStreamSpliceResult { bytes_spliced: 0, success: false },
                    Some("WASI I/O: splice error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn blocking_splice(
        &mut self,
        output: Resource<OutputStream>,
        input: Resource<InputStream>,
        len: u64,
    ) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        debug!("wasi:io/streams output-stream.blocking-splice: len={}", len);

        self.record_handler_event(
            "wasi:io/streams/output-stream.blocking-splice".to_string(),
            IoEventData::OutputStreamSpliceCall { len },
            Some(format!("WASI I/O: blocking splice up to {} bytes", len)),
        );

        // For in-memory streams, blocking-splice is the same as splice
        let result = {
            let table = self.resource_table.lock().unwrap();
            let output_stream: &OutputStream = table.get(&output)?;
            let input_stream: &InputStream = table.get(&input)?;
            output_stream.splice(input_stream, len)
        };

        match result {
            Ok(bytes_spliced) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-splice".to_string(),
                    IoEventData::OutputStreamSpliceResult { bytes_spliced, success: true },
                    Some(format!("WASI I/O: blocking spliced {} bytes", bytes_spliced)),
                );
                Ok(Ok(bytes_spliced))
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:io/streams/output-stream.blocking-splice".to_string(),
                    IoEventData::OutputStreamSpliceResult { bytes_spliced: 0, success: false },
                    Some("WASI I/O: blocking splice error".to_string()),
                );
                Ok(Err(to_bindings_stream_error(e)))
            }
        }
    }

    async fn drop(&mut self, stream: Resource<OutputStream>) -> Result<()> {
        debug!("wasi:io/streams output-stream.drop");

        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(stream);
        Ok(())
    }
}

// =============================================================================
// wasi:cli Host implementations
// =============================================================================

impl<E> StdinHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn get_stdin(&mut self) -> Result<Resource<InputStream>> {
        debug!("wasi:cli/stdin get-stdin");

        self.record_handler_event(
            "wasi:cli/stdin/get-stdin".to_string(),
            IoEventData::StdinGetCall,
            Some("WASI CLI: getting stdin stream".to_string()),
        );

        // Create an empty stdin stream (closed, no input available)
        let stdin_stream = InputStream::closed();
        let resource = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(stdin_stream)?
        };

        self.record_handler_event(
            "wasi:cli/stdin/get-stdin".to_string(),
            IoEventData::StdinGetResult,
            Some("WASI CLI: stdin stream created".to_string()),
        );

        Ok(resource)
    }
}

impl<E> StdoutHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn get_stdout(&mut self) -> Result<Resource<OutputStream>> {
        debug!("wasi:cli/stdout get-stdout");

        self.record_handler_event(
            "wasi:cli/stdout/get-stdout".to_string(),
            IoEventData::StdoutGetCall,
            Some("WASI CLI: getting stdout stream".to_string()),
        );

        let stdout_stream = OutputStream::new();
        let resource = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(stdout_stream)?
        };

        self.record_handler_event(
            "wasi:cli/stdout/get-stdout".to_string(),
            IoEventData::StdoutGetResult,
            Some("WASI CLI: stdout stream created".to_string()),
        );

        Ok(resource)
    }
}

impl<E> StderrHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn get_stderr(&mut self) -> Result<Resource<OutputStream>> {
        debug!("wasi:cli/stderr get-stderr");

        self.record_handler_event(
            "wasi:cli/stderr/get-stderr".to_string(),
            IoEventData::StderrGetCall,
            Some("WASI CLI: getting stderr stream".to_string()),
        );

        let stderr_stream = OutputStream::new();
        let resource = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(stderr_stream)?
        };

        self.record_handler_event(
            "wasi:cli/stderr/get-stderr".to_string(),
            IoEventData::StderrGetResult,
            Some("WASI CLI: stderr stream created".to_string()),
        );

        Ok(resource)
    }
}

impl<E> EnvironmentHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn get_environment(&mut self) -> Result<Vec<(String, String)>> {
        debug!("wasi:cli/environment get-environment");

        self.record_handler_event(
            "wasi:cli/environment/get-environment".to_string(),
            IoEventData::EnvironmentGetCall,
            Some("WASI CLI: getting environment variables".to_string()),
        );

        // Return empty environment for sandboxed actors
        let env_vars = Vec::new();

        self.record_handler_event(
            "wasi:cli/environment/get-environment".to_string(),
            IoEventData::EnvironmentGetResult { count: 0 },
            Some("WASI CLI: returned empty environment".to_string()),
        );

        Ok(env_vars)
    }

    async fn get_arguments(&mut self) -> Result<Vec<String>> {
        debug!("wasi:cli/environment get-arguments");

        self.record_handler_event(
            "wasi:cli/environment/get-arguments".to_string(),
            IoEventData::ArgumentsGetCall,
            Some("WASI CLI: getting command line arguments".to_string()),
        );

        // Try to get arguments from extensions, or return empty
        let args = self
            .get_extension::<CliArguments>()
            .map(|a| a.0)
            .unwrap_or_default();

        let count = args.len();
        self.record_handler_event(
            "wasi:cli/environment/get-arguments".to_string(),
            IoEventData::ArgumentsGetResult { count },
            Some(format!("WASI CLI: returned {} arguments", count)),
        );

        Ok(args)
    }

    async fn initial_cwd(&mut self) -> Result<Option<String>> {
        debug!("wasi:cli/environment initial-cwd");

        self.record_handler_event(
            "wasi:cli/environment/initial-cwd".to_string(),
            IoEventData::InitialCwdCall,
            Some("WASI CLI: getting initial working directory".to_string()),
        );

        // Try to get initial cwd from extensions, or return None
        let cwd = self
            .get_extension::<InitialCwd>()
            .and_then(|c| c.0);

        self.record_handler_event(
            "wasi:cli/environment/initial-cwd".to_string(),
            IoEventData::InitialCwdResult { cwd: cwd.clone() },
            Some(format!("WASI CLI: initial cwd = {:?}", cwd)),
        );

        Ok(cwd)
    }
}

impl<E> ExitHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn exit(&mut self, status: Result<(), ()>) -> Result<()> {
        let status_code = match status {
            Ok(()) => 0,
            Err(()) => 1,
        };

        debug!("wasi:cli/exit exit: status={}", status_code);

        self.record_handler_event(
            "wasi:cli/exit/exit".to_string(),
            IoEventData::ExitCall { status: status_code },
            Some(format!("WASI CLI: exit called with status {}", status_code)),
        );

        // For now, just log the exit - don't actually terminate the actor
        warn!("Actor called exit with status {}", status_code);

        Ok(())
    }
}

// Terminal interfaces - we don't provide actual terminal access
impl<E> TerminalInputHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    // No methods - just a resource type
}

impl<E> HostTerminalInput for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn drop(&mut self, _rep: Resource<crate::bindings::wasi::cli::terminal_input::TerminalInput>) -> Result<()> {
        // Terminal resources are never actually created
        Ok(())
    }
}

impl<E> TerminalOutputHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    // No methods - just a resource type
}

impl<E> HostTerminalOutput for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn drop(&mut self, _rep: Resource<crate::bindings::wasi::cli::terminal_output::TerminalOutput>) -> Result<()> {
        // Terminal resources are never actually created
        Ok(())
    }
}

impl<E> TerminalStdinHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn get_terminal_stdin(&mut self) -> Result<Option<Resource<crate::bindings::wasi::cli::terminal_input::TerminalInput>>> {
        debug!("wasi:cli/terminal-stdin get-terminal-stdin");
        // Theater actors don't have access to real terminals
        Ok(None)
    }
}

impl<E> TerminalStdoutHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn get_terminal_stdout(&mut self) -> Result<Option<Resource<crate::bindings::wasi::cli::terminal_output::TerminalOutput>>> {
        debug!("wasi:cli/terminal-stdout get-terminal-stdout");
        // Theater actors don't have access to real terminals
        Ok(None)
    }
}

impl<E> TerminalStderrHost for ActorStore<E>
where
    E: EventPayload + Clone + From<IoEventData> + Send,
{
    async fn get_terminal_stderr(&mut self) -> Result<Option<Resource<crate::bindings::wasi::cli::terminal_output::TerminalOutput>>> {
        debug!("wasi:cli/terminal-stderr get-terminal-stderr");
        // Theater actors don't have access to real terminals
        Ok(None)
    }
}
