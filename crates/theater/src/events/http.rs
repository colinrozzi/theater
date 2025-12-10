use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpEventData {
    // HTTP framework server lifecycle events
    ServerCreate {
        server_id: u64,
        host: String,
        port: u16,
        with_tls: bool,
    },
    ServerStart {
        server_id: u64,
        port: u16,
    },
    ServerStartHttps {
        server_id: u64,
        port: u16,
        cert_path: String,
    },
    ServerStop {
        server_id: u64,
    },
    ServerDestroy {
        server_id: u64,
    },

    // HTTP framework route events
    RouteAdd {
        route_id: u64,
        server_id: u64,
        path: String,
        method: String,
        handler_id: u64,
    },
    RouteRemove {
        route_id: u64,
        server_id: u64,
    },

    // HTTP framework middleware events
    MiddlewareAdd {
        middleware_id: u64,
        server_id: u64,
        path: String,
        handler_id: u64,
    },
    MiddlewareRemove {
        middleware_id: u64,
        server_id: u64,
    },

    // HTTP framework WebSocket events
    WebSocketEnable {
        server_id: u64,
        path: String,
        connect_handler_id: Option<u64>,
        message_handler_id: u64,
        disconnect_handler_id: Option<u64>,
    },
    WebSocketDisable {
        server_id: u64,
        path: String,
    },
    WebSocketConnect {
        server_id: u64,
        connection_id: u64,
        path: String,
    },
    WebSocketDisconnect {
        server_id: u64,
        connection_id: u64,
    },
    WebSocketMessage {
        server_id: u64,
        connection_id: u64,
        message_type: String,
        message_size: usize,
    },

    // HTTP framework handler events
    HandlerRegister {
        handler_id: u64,
        name: String,
    },
    HandlerInvoke {
        handler_id: u64,
        handler_type: String,
    },

    // Original Request handling events
    HttpRequestCall {
        method: String,
        path: String,
        headers_count: usize,
        body_size: usize,
    },
    HttpRequestResult {
        status: u16,
        headers_count: usize,
        body_size: usize,
        success: bool,
    },

    // Client request events
    HttpClientRequestCall {
        method: String,
        url: String,
        headers_count: usize,
        body: Option<String>,
    },
    HttpClientRequestResult {
        status: u16,
        headers_count: usize,
        success: bool,
        body: Option<String>,
    },

    // TLS-specific events
    TlsValidationStart {
        server_id: u64,
        cert_path: String,
        key_path: String,
    },
    TlsValidationSuccess {
        server_id: u64,
    },
    TlsCertificateLoad {
        server_id: u64,
        cert_path: String,
        cert_count: usize,
    },
    TlsPrivateKeyLoad {
        server_id: u64,
        key_path: String,
    },
    TlsConfigCreated {
        server_id: u64,
    },
    TlsHandshakeError {
        server_id: u64,
        error: String,
    },

    // Error events
    Error {
        operation: String,
        path: String,
        message: String,
    },

    // Permission events
    PermissionDenied {
        operation: String,
        method: String,
        url: String,
        reason: String,
    },

    // Handler setup events
    HandlerSetupStart,
    HandlerSetupSuccess,
    HandlerSetupError {
        error: String,
        step: String,
    },
    LinkerInstanceSuccess,
    FunctionSetupStart {
        function_name: String,
    },
    FunctionSetupSuccess {
        function_name: String,
    },
}

pub struct HttpEvent {
    pub data: HttpEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
