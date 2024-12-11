use crate::http::HttpHost;
use tokio::sync::mpsc;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct Store {
    pub http: Option<HttpHost>,
}

impl Store {
    pub fn new() -> Self {
        Self { http: None }
    }

    pub fn with_http(port: u16, mailbox_tx: mpsc::Sender<crate::ActorMessage>) -> Self {
        Self {
            http: Some(HttpHost::new(port, mailbox_tx)),
        }
    }
}

