use crate::http::HttpHost;
use tracing::info;
use tokio::sync::mpsc;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct Store {
    pub http: Option<HttpHost>,
    pub http_server: Option<HttpHost>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            http: None,
            http_server: None,
        }
    }

    pub fn with_http(port: u16, mailbox_tx: mpsc::Sender<crate::ActorMessage>) -> Self {
        info!("[STORE] Initializing store with HTTP on port {}", port);
        Self {
            http: Some(HttpHost::new(mailbox_tx)),
            http_server: None,
        }
    }

    pub fn with_both_http(
        http_port: u16,
        http_server_port: u16,
        mailbox_tx: mpsc::Sender<crate::ActorMessage>,
    ) -> Self {
        info!(
            "[STORE] Initializing store with HTTP on port {} and HTTP server on port {}",
            http_port, http_server_port
        );
        Self {
            http: Some(HttpHost::new(mailbox_tx.clone())),
            http_server: Some(HttpHost::new(mailbox_tx)),
        }
    }

    pub fn http_port(&self) -> Option<u16> {
        self.http.as_ref().map(|_| 8080)
    }

    pub fn http_server_port(&self) -> Option<u16> {
        self.http_server.as_ref().map(|_| 8081)
    }
}
