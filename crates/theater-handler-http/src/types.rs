//! WASI HTTP types implementation
//!
//! Provides backing types for WASI HTTP resources. These types are mapped
//! to the WIT-defined resources via bindgen's `with` option.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::component::{ComponentType, Lift, Lower};
use theater_handler_io::{InputStream, OutputStream};

/// WASI HTTP method variant - matches wasi:http/types@0.2.0 method
#[derive(Debug, Clone, PartialEq, Eq, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum WasiMethod {
    #[component(name = "get")]
    Get,
    #[component(name = "head")]
    Head,
    #[component(name = "post")]
    Post,
    #[component(name = "put")]
    Put,
    #[component(name = "delete")]
    Delete,
    #[component(name = "connect")]
    Connect,
    #[component(name = "options")]
    Options,
    #[component(name = "trace")]
    Trace,
    #[component(name = "patch")]
    Patch,
    #[component(name = "other")]
    Other(String),
}

impl WasiMethod {
    pub fn as_str(&self) -> &str {
        match self {
            WasiMethod::Get => "GET",
            WasiMethod::Head => "HEAD",
            WasiMethod::Post => "POST",
            WasiMethod::Put => "PUT",
            WasiMethod::Delete => "DELETE",
            WasiMethod::Connect => "CONNECT",
            WasiMethod::Options => "OPTIONS",
            WasiMethod::Trace => "TRACE",
            WasiMethod::Patch => "PATCH",
            WasiMethod::Other(s) => s.as_str(),
        }
    }
}

/// WASI HTTP scheme variant - matches wasi:http/types@0.2.0 scheme
#[derive(Debug, Clone, PartialEq, Eq, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum WasiScheme {
    #[component(name = "HTTP")]
    Http,
    #[component(name = "HTTPS")]
    Https,
    #[component(name = "other")]
    Other(String),
}

impl WasiScheme {
    pub fn as_str(&self) -> &str {
        match self {
            WasiScheme::Http => "http",
            WasiScheme::Https => "https",
            WasiScheme::Other(s) => s.as_str(),
        }
    }
}

/// WASI HTTP header-error variant - matches wasi:http/types@0.2.0 header-error
#[derive(Debug, Clone, PartialEq, Eq, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum WasiHeaderError {
    #[component(name = "invalid-syntax")]
    InvalidSyntax,
    #[component(name = "forbidden")]
    Forbidden,
    #[component(name = "immutable")]
    Immutable,
}

/// DNS error payload - matches wasi:http/types@0.2.0 DNS-error-payload
#[derive(Debug, Clone, PartialEq, Eq, ComponentType, Lift, Lower)]
#[component(record)]
pub struct DnsErrorPayload {
    pub rcode: Option<String>,
    #[component(name = "info-code")]
    pub info_code: Option<u16>,
}

/// TLS alert received payload - matches wasi:http/types@0.2.0 TLS-alert-received-payload
#[derive(Debug, Clone, PartialEq, Eq, ComponentType, Lift, Lower)]
#[component(record)]
pub struct TlsAlertReceivedPayload {
    #[component(name = "alert-id")]
    pub alert_id: Option<u8>,
    #[component(name = "alert-message")]
    pub alert_message: Option<String>,
}

/// Field size payload - matches wasi:http/types@0.2.0 field-size-payload
#[derive(Debug, Clone, PartialEq, Eq, ComponentType, Lift, Lower)]
#[component(record)]
pub struct FieldSizePayload {
    #[component(name = "field-name")]
    pub field_name: Option<String>,
    #[component(name = "field-size")]
    pub field_size: Option<u32>,
}

/// WASI HTTP error-code variant - matches wasi:http/types@0.2.0 error-code
#[derive(Debug, Clone, PartialEq, Eq, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum WasiErrorCode {
    #[component(name = "DNS-timeout")]
    DnsTimeout,
    #[component(name = "DNS-error")]
    DnsError(DnsErrorPayload),
    #[component(name = "destination-not-found")]
    DestinationNotFound,
    #[component(name = "destination-unavailable")]
    DestinationUnavailable,
    #[component(name = "destination-IP-prohibited")]
    DestinationIpProhibited,
    #[component(name = "destination-IP-unroutable")]
    DestinationIpUnroutable,
    #[component(name = "connection-refused")]
    ConnectionRefused,
    #[component(name = "connection-terminated")]
    ConnectionTerminated,
    #[component(name = "connection-timeout")]
    ConnectionTimeout,
    #[component(name = "connection-read-timeout")]
    ConnectionReadTimeout,
    #[component(name = "connection-write-timeout")]
    ConnectionWriteTimeout,
    #[component(name = "connection-limit-reached")]
    ConnectionLimitReached,
    #[component(name = "TLS-protocol-error")]
    TlsProtocolError,
    #[component(name = "TLS-certificate-error")]
    TlsCertificateError,
    #[component(name = "TLS-alert-received")]
    TlsAlertReceived(TlsAlertReceivedPayload),
    #[component(name = "HTTP-request-denied")]
    HttpRequestDenied,
    #[component(name = "HTTP-request-length-required")]
    HttpRequestLengthRequired,
    #[component(name = "HTTP-request-body-size")]
    HttpRequestBodySize(Option<u64>),
    #[component(name = "HTTP-request-method-invalid")]
    HttpRequestMethodInvalid,
    #[component(name = "HTTP-request-URI-invalid")]
    HttpRequestUriInvalid,
    #[component(name = "HTTP-request-URI-too-long")]
    HttpRequestUriTooLong,
    #[component(name = "HTTP-request-header-section-size")]
    HttpRequestHeaderSectionSize(Option<u32>),
    #[component(name = "HTTP-request-header-size")]
    HttpRequestHeaderSize(Option<FieldSizePayload>),
    #[component(name = "HTTP-request-trailer-section-size")]
    HttpRequestTrailerSectionSize(Option<u32>),
    #[component(name = "HTTP-request-trailer-size")]
    HttpRequestTrailerSize(FieldSizePayload),
    #[component(name = "HTTP-response-incomplete")]
    HttpResponseIncomplete,
    #[component(name = "HTTP-response-header-section-size")]
    HttpResponseHeaderSectionSize(Option<u32>),
    #[component(name = "HTTP-response-header-size")]
    HttpResponseHeaderSize(FieldSizePayload),
    #[component(name = "HTTP-response-body-size")]
    HttpResponseBodySize(Option<u64>),
    #[component(name = "HTTP-response-trailer-section-size")]
    HttpResponseTrailerSectionSize(Option<u32>),
    #[component(name = "HTTP-response-trailer-size")]
    HttpResponseTrailerSize(FieldSizePayload),
    #[component(name = "HTTP-response-transfer-coding")]
    HttpResponseTransferCoding(Option<String>),
    #[component(name = "HTTP-response-content-coding")]
    HttpResponseContentCoding(Option<String>),
    #[component(name = "HTTP-response-timeout")]
    HttpResponseTimeout,
    #[component(name = "HTTP-upgrade-failed")]
    HttpUpgradeFailed,
    #[component(name = "HTTP-protocol-error")]
    HttpProtocolError,
    #[component(name = "loop-detected")]
    LoopDetected,
    #[component(name = "configuration-error")]
    ConfigurationError,
    #[component(name = "internal-error")]
    InternalError(Option<String>),
}

/// Internal HTTP method (for our own use)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Connect,
    Trace,
    Other(String),
}

impl Method {
    pub fn from_wasi(wasi: &WasiMethod) -> Self {
        match wasi {
            WasiMethod::Get => Method::Get,
            WasiMethod::Head => Method::Head,
            WasiMethod::Post => Method::Post,
            WasiMethod::Put => Method::Put,
            WasiMethod::Delete => Method::Delete,
            WasiMethod::Connect => Method::Connect,
            WasiMethod::Options => Method::Options,
            WasiMethod::Trace => Method::Trace,
            WasiMethod::Patch => Method::Patch,
            WasiMethod::Other(s) => Method::Other(s.clone()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Connect => "CONNECT",
            Method::Trace => "TRACE",
            Method::Other(s) => s.as_str(),
        }
    }
}

/// Internal HTTP scheme (for our own use)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
    Other(String),
}

impl Scheme {
    pub fn from_wasi(wasi: &WasiScheme) -> Self {
        match wasi {
            WasiScheme::Http => Scheme::Http,
            WasiScheme::Https => Scheme::Https,
            WasiScheme::Other(s) => Scheme::Other(s.clone()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
            Scheme::Other(s) => s.as_str(),
        }
    }
}

/// HTTP headers resource
#[derive(Debug, Clone)]
pub struct Headers {
    inner: Arc<Mutex<HeadersInner>>,
}

#[derive(Debug)]
struct HeadersInner {
    headers: HashMap<String, Vec<Vec<u8>>>,
}

impl Headers {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HeadersInner {
                headers: HashMap::new(),
            })),
        }
    }

    pub fn from_map(headers: HashMap<String, Vec<Vec<u8>>>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HeadersInner { headers })),
        }
    }

    /// Get all values for a header name
    pub fn get(&self, name: &str) -> Vec<Vec<u8>> {
        let inner = self.inner.lock().unwrap();
        inner.headers.get(&name.to_lowercase()).cloned().unwrap_or_default()
    }

    /// Set a header value (replaces existing)
    pub fn set(&self, name: &str, value: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        inner.headers.insert(name.to_lowercase(), vec![value]);
    }

    /// Append a header value
    pub fn append(&self, name: &str, value: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        inner.headers.entry(name.to_lowercase())
            .or_insert_with(Vec::new)
            .push(value);
    }

    /// Delete a header
    pub fn delete(&self, name: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.headers.remove(&name.to_lowercase());
    }

    /// Get all header entries
    pub fn entries(&self) -> Vec<(String, Vec<Vec<u8>>)> {
        let inner = self.inner.lock().unwrap();
        inner.headers.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

impl Default for Headers {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP status code
pub type StatusCode = u16;

/// Incoming HTTP request resource
#[derive(Debug)]
pub struct IncomingRequest {
    pub method: Method,
    pub scheme: Option<Scheme>,
    pub authority: Option<String>,
    pub path_with_query: Option<String>,
    pub headers: Headers,
    // Body will be a wasi:io/streams input-stream resource
}

/// Outgoing HTTP request resource
#[derive(Debug)]
pub struct OutgoingRequest {
    pub method: Method,
    pub scheme: Option<Scheme>,
    pub authority: Option<String>,
    pub path_with_query: Option<String>,
    pub headers: Headers,
    // Body will be a wasi:io/streams output-stream resource
}

/// Incoming HTTP response resource
#[derive(Debug)]
pub struct IncomingResponse {
    pub status: StatusCode,
    pub headers: Headers,
    // Body will be a wasi:io/streams input-stream resource
}

/// Outgoing HTTP response resource
#[derive(Debug)]
pub struct OutgoingResponse {
    pub status: StatusCode,
    pub headers: Headers,
    // Body will be a wasi:io/streams output-stream resource
}

/// Request options for outgoing requests
#[derive(Debug, Clone)]
pub struct RequestOptions {
    pub connect_timeout: Option<std::time::Duration>,
    pub first_byte_timeout: Option<std::time::Duration>,
    pub between_bytes_timeout: Option<std::time::Duration>,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Some(std::time::Duration::from_secs(30)),
            first_byte_timeout: Some(std::time::Duration::from_secs(60)),
            between_bytes_timeout: Some(std::time::Duration::from_secs(30)),
        }
    }
}

// ============================================================================
// Host backing types for bindgen resource mapping
// ============================================================================

/// Backing type for wasi:http/types.fields resource
#[derive(Debug, Clone)]
pub struct HostFields {
    entries: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    immutable: bool,
}

impl HostFields {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            immutable: false,
        }
    }

    pub fn from_list(entries: Vec<(String, Vec<u8>)>) -> Self {
        Self {
            entries: Arc::new(Mutex::new(entries)),
            immutable: false,
        }
    }

    pub fn immutable(entries: Vec<(String, Vec<u8>)>) -> Self {
        Self {
            entries: Arc::new(Mutex::new(entries)),
            immutable: true,
        }
    }

    pub fn get(&self, name: &str) -> Vec<Vec<u8>> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
            .collect()
    }

    pub fn has(&self, name: &str) -> bool {
        let entries = self.entries.lock().unwrap();
        entries.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
    }

    pub fn set(&self, name: &str, values: Vec<Vec<u8>>) -> bool {
        if self.immutable {
            return false;
        }
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        for value in values {
            entries.push((name.to_string(), value));
        }
        true
    }

    pub fn delete(&self, name: &str) -> bool {
        if self.immutable {
            return false;
        }
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        true
    }

    pub fn append(&self, name: &str, value: Vec<u8>) -> bool {
        if self.immutable {
            return false;
        }
        let mut entries = self.entries.lock().unwrap();
        entries.push((name.to_string(), value));
        true
    }

    pub fn entries(&self) -> Vec<(String, Vec<u8>)> {
        let entries = self.entries.lock().unwrap();
        entries.clone()
    }

    pub fn clone_fields(&self) -> Self {
        let entries = self.entries.lock().unwrap();
        Self {
            entries: Arc::new(Mutex::new(entries.clone())),
            immutable: false,
        }
    }

    pub fn is_immutable(&self) -> bool {
        self.immutable
    }
}

impl Default for HostFields {
    fn default() -> Self {
        Self::new()
    }
}

/// Backing type for wasi:http/types.outgoing-request resource
#[derive(Debug)]
pub struct HostOutgoingRequest {
    pub method: Method,
    pub scheme: Option<Scheme>,
    pub authority: Option<String>,
    pub path_with_query: Option<String>,
    pub headers: HostFields,
    pub body: Option<HostOutgoingBody>,
}

impl HostOutgoingRequest {
    pub fn new(headers: HostFields) -> Self {
        Self {
            method: Method::Get,
            scheme: None,
            authority: None,
            path_with_query: None,
            headers,
            body: None,
        }
    }
}

/// Backing type for wasi:http/types.outgoing-response resource
#[derive(Debug)]
pub struct HostOutgoingResponse {
    pub status: u16,
    pub headers: HostFields,
    pub body: Option<HostOutgoingBody>,
}

impl HostOutgoingResponse {
    pub fn new(headers: HostFields) -> Self {
        Self {
            status: 200,
            headers,
            body: None,
        }
    }
}

/// Backing type for wasi:http/types.incoming-request resource
#[derive(Debug)]
pub struct HostIncomingRequest {
    pub method: Method,
    pub scheme: Option<Scheme>,
    pub authority: Option<String>,
    pub path_with_query: Option<String>,
    pub headers: HostFields,
    pub body: Option<HostIncomingBody>,
}

/// Backing type for wasi:http/types.incoming-response resource
#[derive(Debug, Clone)]
pub struct HostIncomingResponse {
    pub status: u16,
    pub headers: HostFields,
    pub body: Option<HostIncomingBody>,
}

/// Backing type for wasi:http/types.outgoing-body resource
#[derive(Debug)]
pub struct HostOutgoingBody {
    pub stream: Option<OutputStream>,
    pub finished: bool,
}

impl HostOutgoingBody {
    pub fn new() -> Self {
        Self {
            stream: Some(OutputStream::new()),
            finished: false,
        }
    }

    pub fn take_stream(&mut self) -> Option<OutputStream> {
        self.stream.take()
    }

    pub fn finish(&mut self) {
        self.finished = true;
    }
}

impl Default for HostOutgoingBody {
    fn default() -> Self {
        Self::new()
    }
}

/// Backing type for wasi:http/types.incoming-body resource
#[derive(Debug, Clone)]
pub struct HostIncomingBody {
    pub stream: Option<InputStream>,
    pub consumed: bool,
}

impl HostIncomingBody {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            stream: Some(InputStream::from_bytes(data)),
            consumed: false,
        }
    }

    pub fn take_stream(&mut self) -> Option<InputStream> {
        self.consumed = true;
        self.stream.take()
    }
}

/// Backing type for wasi:http/types.response-outparam resource
pub struct HostResponseOutparam {
    pub sender: Option<tokio::sync::oneshot::Sender<ResponseOutparamResult>>,
}

/// Result passed through response-outparam
pub enum ResponseOutparamResult {
    Response(HostOutgoingResponse),
    Error(WasiErrorCode),
}

/// Backing type for wasi:http/types.future-incoming-response resource
pub struct HostFutureIncomingResponse {
    pub receiver: Option<tokio::sync::oneshot::Receiver<Result<HostIncomingResponse, WasiErrorCode>>>,
    pub response: Option<Result<HostIncomingResponse, WasiErrorCode>>,
}

/// Backing type for wasi:http/types.future-trailers resource
pub struct HostFutureTrailers {
    pub trailers: Option<Result<Option<HostFields>, WasiErrorCode>>,
}

/// Backing type for wasi:http/types.request-options resource
#[derive(Debug, Clone, Default)]
pub struct HostRequestOptions {
    pub connect_timeout: Option<u64>,
    pub first_byte_timeout: Option<u64>,
    pub between_bytes_timeout: Option<u64>,
}
