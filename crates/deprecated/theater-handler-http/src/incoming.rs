//! WASI HTTP incoming-handler implementation
//!
//! Provides HTTP server capability using hyper.

use crate::types::*;
use theater_handler_io::{InputStream, OutputStream};
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;

/// Incoming HTTP body resource backed by an input stream
#[derive(Debug, Clone)]
pub struct IncomingBody {
    stream: Arc<Mutex<Option<InputStream>>>,
}

impl IncomingBody {
    pub fn new() -> Self {
        Self {
            stream: Arc::new(Mutex::new(None)),
        }
    }

    pub fn from_stream(stream: InputStream) -> Self {
        Self {
            stream: Arc::new(Mutex::new(Some(stream))),
        }
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            stream: Arc::new(Mutex::new(Some(InputStream::from_bytes(data)))),
        }
    }

    /// Get the input stream for this body (can only be called once)
    pub fn stream(&self) -> Option<InputStream> {
        let mut guard = self.stream.lock().unwrap();
        guard.take()
    }
}

impl Default for IncomingBody {
    fn default() -> Self {
        Self::new()
    }
}

/// Outgoing HTTP body resource backed by an output stream
#[derive(Debug, Clone)]
pub struct OutgoingBody {
    stream: Arc<Mutex<Option<OutputStream>>>,
    finished: Arc<Mutex<bool>>,
}

impl OutgoingBody {
    pub fn new() -> Self {
        Self {
            stream: Arc::new(Mutex::new(Some(OutputStream::new()))),
            finished: Arc::new(Mutex::new(false)),
        }
    }

    /// Get the output stream for this body (can only be called once)
    pub fn stream(&self) -> Option<OutputStream> {
        let mut guard = self.stream.lock().unwrap();
        guard.take()
    }

    /// Get a reference to the output stream without taking ownership
    pub fn stream_ref(&self) -> Option<OutputStream> {
        let guard = self.stream.lock().unwrap();
        guard.clone()
    }

    /// Get the contents written to this body
    pub fn get_contents(&self) -> Vec<u8> {
        let guard = self.stream.lock().unwrap();
        if let Some(stream) = &*guard {
            stream.get_contents()
        } else {
            Vec::new()
        }
    }

    /// Mark the body as finished
    pub fn finish(&self) {
        let mut guard = self.finished.lock().unwrap();
        *guard = true;
    }

    /// Check if the body has been finished
    pub fn is_finished(&self) -> bool {
        let guard = self.finished.lock().unwrap();
        *guard
    }
}

impl Default for OutgoingBody {
    fn default() -> Self {
        Self::new()
    }
}

/// Outgoing HTTP response for incoming-handler
/// This is what the component creates to respond to incoming requests
#[derive(Debug, Clone)]
pub struct OutgoingResponseResource {
    pub status: u16,
    pub headers: Headers,
    pub body: Arc<Mutex<Option<OutgoingBody>>>,
}

impl OutgoingResponseResource {
    pub fn new(headers: Headers) -> Self {
        Self {
            status: 200,
            headers,
            body: Arc::new(Mutex::new(Some(OutgoingBody::new()))),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn set_status(&mut self, status: u16) {
        self.status = status;
    }

    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// Get the body (can only be called once)
    pub fn take_body(&self) -> Option<OutgoingBody> {
        let mut guard = self.body.lock().unwrap();
        guard.take()
    }
}

/// Response sent back via the outparam
#[derive(Debug)]
pub enum ResponseOutparamResult {
    Response(Resource<OutgoingResponseResource>),
    Error(WasiErrorCode),
}

/// Response outparam - used to send the response back
/// The component calls response-outparam.set() to provide its response
#[derive(Debug)]
pub struct ResponseOutparam {
    sender: Arc<Mutex<Option<tokio::sync::oneshot::Sender<ResponseOutparamResult>>>>,
}

impl ResponseOutparam {
    pub fn new(sender: tokio::sync::oneshot::Sender<ResponseOutparamResult>) -> Self {
        Self {
            sender: Arc::new(Mutex::new(Some(sender))),
        }
    }

    /// Set the response (consumes the sender)
    pub fn set(&self, result: ResponseOutparamResult) -> bool {
        let mut guard = self.sender.lock().unwrap();
        if let Some(sender) = guard.take() {
            sender.send(result).is_ok()
        } else {
            false
        }
    }
}

impl Clone for ResponseOutparam {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

/// Incoming HTTP request resource for incoming-handler
/// This represents an HTTP request received by the server
#[derive(Debug, Clone)]
pub struct IncomingRequestResource {
    pub method: WasiMethod,
    pub scheme: Option<WasiScheme>,
    pub authority: Option<String>,
    pub path_with_query: Option<String>,
    pub headers: Headers,
    pub body: Arc<Mutex<Option<IncomingBody>>>,
}

impl IncomingRequestResource {
    pub fn new(
        method: WasiMethod,
        scheme: Option<WasiScheme>,
        authority: Option<String>,
        path_with_query: Option<String>,
        headers: Headers,
        body: Vec<u8>,
    ) -> Self {
        Self {
            method,
            scheme,
            authority,
            path_with_query,
            headers,
            body: Arc::new(Mutex::new(Some(IncomingBody::from_bytes(body)))),
        }
    }

    /// Consume the body (can only be called once)
    pub fn consume(&self) -> Option<IncomingBody> {
        let mut guard = self.body.lock().unwrap();
        guard.take()
    }
}
