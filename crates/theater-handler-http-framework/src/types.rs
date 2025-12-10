use serde::{Deserialize, Serialize};
use wasmtime::component::{ComponentType, Lift, Lower};

// Server configuration and info types
#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct ServerConfig {
    pub port: Option<u16>,
    pub host: Option<String>,
    #[component(name = "tls-config")]
    pub tls_config: Option<TlsConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct TlsConfig {
    #[component(name = "cert-path")]
    pub cert_path: String,
    #[component(name = "key-path")]
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct ServerInfo {
    pub id: u64,
    pub port: u16,
    pub host: String,
    pub running: bool,
    #[component(name = "routes-count")]
    pub routes_count: u32,
    #[component(name = "middleware-count")]
    pub middleware_count: u32,
    #[component(name = "websocket-enabled")]
    pub websocket_enabled: bool,
}

// HTTP request and response types
#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct HttpRequest {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

// Middleware result type
#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct MiddlewareResult {
    pub proceed: bool,
    pub request: HttpRequest,
}

// WebSocket message types
#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum MessageType {
    #[component(name = "text")]
    Text,
    #[component(name = "binary")]
    Binary,
    #[component(name = "connect")]
    Connect,
    #[component(name = "close")]
    Close,
    #[component(name = "ping")]
    Ping,
    #[component(name = "pong")]
    Pong,
    #[component(name = "other")]
    Other(String),
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct WebSocketMessage {
    pub ty: MessageType,
    pub data: Option<Vec<u8>>,
    pub text: Option<String>,
}

// Helper functions
pub fn is_valid_host(host: &str) -> bool {
    // Basic validation - hostname or IP, not empty, no spaces, reasonable length
    !host.is_empty() && !host.contains(' ') && host.len() < 255
}

pub fn is_valid_method(method: &str) -> bool {
    matches!(
        method,
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" | "TRACE" | "CONNECT"
        // WebDAV methods for git-http-push and WebDAV protocol support
        | "LOCK" | "UNLOCK" | "MKCOL" | "COPY" | "MOVE" | "PROPFIND" | "PROPPATCH"
    )
}
