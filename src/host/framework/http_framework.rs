use crate::actor_handle::ActorHandle;
use crate::actor_store::ActorStore;
use crate::events::http::HttpEventData;
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};

use anyhow::Result;

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use thiserror::Error;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::handlers::{HandlerConfig, HandlerRegistry, HandlerType};
use super::server_instance::ServerInstance;
use super::types::*;

#[derive(Error, Debug)]
pub enum HttpFrameworkError {
    #[error("Server not found: {0}")]
    ServerNotFound(u64),

    #[error("Handler not found: {0}")]
    HandlerNotFound(u64),

    #[error("Route not found: {0}")]
    RouteNotFound(u64),

    #[error("Middleware not found: {0}")]
    MiddlewareNotFound(u64),

    #[error("WebSocket not enabled on path: {0}")]
    WebSocketNotEnabled(String),

    #[error("WebSocket connection not found: {0}")]
    WebSocketConnectionNotFound(u64),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

struct ServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct HttpFramework {
    servers: Arc<RwLock<HashMap<u64, ServerInstance>>>,
    handlers: Arc<RwLock<HandlerRegistry>>,
    next_server_id: Arc<AtomicU64>,
    next_handler_id: Arc<AtomicU64>,
    next_route_id: Arc<AtomicU64>,
    next_middleware_id: Arc<AtomicU64>,
    server_handles: Arc<RwLock<HashMap<u64, ServerHandle>>>,
}

impl HttpFramework {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            next_server_id: Arc::new(AtomicU64::new(1)),
            next_handler_id: Arc::new(AtomicU64::new(1)),
            next_route_id: Arc::new(AtomicU64::new(1)),
            next_middleware_id: Arc::new(AtomicU64::new(1)),
            server_handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up HTTP framework host functions");

        let servers_clone = self.servers.clone();
        let handlers_clone = self.handlers.clone();
        let next_server_id = self.next_server_id.clone();
        let next_handler_id = self.next_handler_id.clone();
        let next_route_id = self.next_route_id.clone();
        let next_middleware_id = self.next_middleware_id.clone();
        let server_handles_clone = self.server_handles.clone();

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/http-framework")
            .expect("could not instantiate http-framework");

        // Create server implementation
        interface.func_wrap(
            "create-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (config,): (ServerConfig,)|
                  -> Result<(Result<u64, String>,)> {
                let server_id = next_server_id.fetch_add(1, Ordering::SeqCst);

                // Extract config values with defaults
                let port = config.port.unwrap_or(0);
                let host = config.host.unwrap_or_else(|| "127.0.0.1".to_string());

                // Validate host
                if !is_valid_host(&host) {
                    return Ok((Err(format!("Invalid host: {}", host)),));
                }

                // Validate TLS config if present
                if let Some(tls) = &config.tls_config {
                    if tls.cert_path.is_empty() || tls.key_path.is_empty() {
                        return Ok((Err("Invalid TLS configuration".to_string()),));
                    }
                }

                // Create server instance (not started yet)
                let server =
                    ServerInstance::new(server_id, port, host.clone(), config.tls_config.clone());

                // Record event in hash chain
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "http-framework/create-server".to_string(),
                    data: EventData::Http(HttpEventData::ServerCreate {
                        server_id,
                        host: host.clone(),
                        port,
                        with_tls: config.tls_config.is_some(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Created HTTP server {} on {}:{}",
                        server_id, host, port
                    )),
                });

                // Store server instance
                let servers_clone_inner = servers_clone.clone();
                tokio::spawn(async move {
                    let mut servers = servers_clone_inner.write().await;
                    servers.insert(server_id, server);
                });

                Ok((Ok(server_id),))
            },
        )?;

        // Get server info implementation
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "get-server-info",
            move |_ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Result<(Result<ServerInfo, String>,)> {
                let servers_clone = servers_clone.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let servers = servers_clone.read().await;

                    if let Some(server) = servers.get(&server_id) {
                        Ok(server.get_info())
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                match tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                }) {
                    Ok(Ok(info)) => Ok((Ok(info),)),
                    Ok(Err(e)) => Ok((Err(e),)),
                    Err(e) => Ok((Err(format!("Internal error: {}", e)),)),
                }
            },
        )?;

        // Other implementations follow the same pattern...
        // ...

        info!("HTTP framework host functions set up");

        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding export functions for HTTP framework");

        actor_instance
            .register_function::<(u64, HttpRequest), (HttpResponse,)>(
                "ntwk:theater/http-handlers",
                "handle-request",
            )
            .expect("Failed to register handle-request function");

        actor_instance
            .register_function::<(u64, HttpRequest), (MiddlewareResult,)>(
                "ntwk:theater/http-handlers",
                "handle-middleware",
            )
            .expect("Failed to register handle-middleware function");

        actor_instance
            .register_function_no_result::<(u64, u64, String, Option<String>)>(
                "ntwk:theater/http-handlers",
                "handle-websocket-connect",
            )
            .expect("Failed to register handle-websocket-connect function");

        actor_instance
            .register_function::<(u64, u64, WebSocketMessage), (Vec<WebSocketMessage>,)>(
                "ntwk:theater/http-handlers",
                "handle-websocket-message",
            )
            .expect("Failed to register handle-websocket-message function");

        actor_instance
            .register_function_no_result::<(u64, u64)>(
                "ntwk:theater/http-handlers",
                "handle-websocket-disconnect",
            )
            .expect("Failed to register handle-websocket-disconnect function");

        info!("Export functions added for HTTP framework");

        Ok(())
    }

    pub async fn start(&self, _actor_handle: ActorHandle, mut shutdown_receiver: ShutdownReceiver) -> Result<()> {
        // Create task to monitor shutdown signal
        let servers_ref = self.servers.clone();
        let server_handles_ref = self.server_handles.clone();
        
        tokio::spawn(async move {
            debug!("HTTP Framework shutdown monitor started");
            
            // Wait for shutdown signal
            shutdown_receiver.wait_for_shutdown().await;
            info!("HTTP Framework received shutdown signal");
            
            // Shut down all servers
            let servers = servers_ref.read().await;
            let mut handles = server_handles_ref.write().await;
            debug!("HTTP Framework shutting down {} servers", servers.len());
            
            for (id, _server) in servers.iter() {
                debug!("Initiating shutdown of HTTP Framework server {}", id);
                
                if let Some(handle) = handles.get_mut(id) {
                    if let Some(tx) = handle.shutdown_tx.take() {
                        debug!("Sending graceful shutdown signal to server {}", id);
                        if let Err(e) = tx.send(()) {
                            warn!("Failed to send shutdown to HTTP server {}: {}", id, e);
                        } else {
                            debug!("Shutdown signal sent to server {}", id);
                        }
                    } else {
                        debug!("No shutdown channel for server {}", id);
                    }
                    
                    // Give a moment for graceful shutdown
                    debug!("Waiting for server {} to shut down gracefully", id);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    
                    // Force abort if still running
                    if let Some(task) = &handle.task {
                        if !task.is_finished() {
                            debug!("Server {} still running after grace period, aborting", id);
                            handle.task.take().map(|t| t.abort());
                            info!("Forcibly aborted HTTP Framework server {}", id);
                        } else {
                            debug!("Server {} shutdown gracefully", id);
                        }
                    } else {
                        debug!("No task handle for server {}", id);
                    }
                }
            }
            
            debug!("HTTP Framework shutdown complete");
        });
        
        Ok(())
    }
}
