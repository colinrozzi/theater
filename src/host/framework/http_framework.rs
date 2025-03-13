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
        let _server_handles_clone = self.server_handles.clone();

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

        // Start server implementation
        let servers_clone = self.servers.clone();
        interface.func_wrap_async(
            "start-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<u16, String>,)>> + Send> {
                let servers_clone = servers_clone.clone();
                let actor_handle = ctx.data().actor_handle.clone();

                Box::new(async move {
                    let mut servers = servers_clone.write().await;

                    if let Some(server) = servers.get_mut(&server_id) {
                        match server.start(actor_handle).await {
                            Ok(port) => {
                                // Record event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/start-server".to_string(),
                                    data: EventData::Http(HttpEventData::ServerStart {
                                        server_id,
                                        port,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Started HTTP server {} on port {}",
                                        server_id, port
                                    )),
                                });

                                Ok((Ok(port),))
                            }
                            Err(e) => {
                                error!("Failed to start server {}: {}", server_id, e);

                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/start-server".to_string(),
                                    data: EventData::Http(HttpEventData::Error {
                                        operation: "start-server".to_string(),
                                        path: format!("server-{}", server_id),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to start HTTP server {}: {}",
                                        server_id, e
                                    )),
                                });

                                Ok((Err(format!("Failed to start server: {}", e)),))
                            }
                        }
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // Stop server implementation
        let servers_clone = self.servers.clone();
        interface.func_wrap_async(
            "stop-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let servers_clone = servers_clone.clone();

                Box::new(async move {
                    let mut servers = servers_clone.write().await;

                    if let Some(server) = servers.get_mut(&server_id) {
                        match server.stop().await {
                            Ok(_) => {
                                // Record event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/stop-server".to_string(),
                                    data: EventData::Http(HttpEventData::ServerStop { server_id }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Stopped HTTP server {}", server_id)),
                                });

                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                error!("Failed to stop server {}: {}", server_id, e);

                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/stop-server".to_string(),
                                    data: EventData::Http(HttpEventData::Error {
                                        operation: "stop-server".to_string(),
                                        path: format!("server-{}", server_id),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to stop HTTP server {}: {}",
                                        server_id, e
                                    )),
                                });

                                Ok((Err(format!("Failed to stop server: {}", e)),))
                            }
                        }
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // Destroy server implementation
        let servers_clone = self.servers.clone();
        interface.func_wrap_async(
            "destroy-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let servers_clone = servers_clone.clone();

                Box::new(async move {
                    let mut servers = servers_clone.write().await;

                    if let Some(server) = servers.get_mut(&server_id) {
                        // Make sure server is stopped first
                        if server.is_running() {
                            match server.stop().await {
                                Ok(_) => {}
                                Err(e) => {
                                    error!(
                                        "Failed to stop server {} during destroy: {}",
                                        server_id, e
                                    );
                                    // Continue with removal anyway
                                }
                            }
                        }

                        // Remove the server
                        servers.remove(&server_id);

                        // Record event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/destroy-server".to_string(),
                            data: EventData::Http(HttpEventData::ServerDestroy { server_id }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Destroyed HTTP server {}", server_id)),
                        });

                        Ok((Ok(()),))
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // Register handler implementation
        let handlers_clone = self.handlers.clone();
        let next_handler_id = self.next_handler_id.clone();
        interface.func_wrap(
            "register-handler",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (handler_name,): (String,)|
                  -> Result<(Result<u64, String>,)> {
                let handlers_clone = handlers_clone.clone();
                let handler_id = next_handler_id.fetch_add(1, Ordering::SeqCst);

                // Validate handler name
                if handler_name.is_empty() {
                    return Ok((Err("Handler name cannot be empty".to_string()),));
                }

                // Create handler config
                let handler_config = HandlerConfig {
                    id: handler_id,
                    name: handler_name.clone(),
                    handler_type: HandlerType::Unknown, // Will be determined when used
                };

                // Record event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "http-framework/register-handler".to_string(),
                    data: EventData::Http(HttpEventData::HandlerRegister {
                        handler_id,
                        name: handler_name.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Registered handler {} with name '{}'",
                        handler_id, handler_name
                    )),
                });

                // Store handler
                tokio::spawn(async move {
                    let mut handlers = handlers_clone.write().await;
                    handlers.register(handler_config);
                });

                Ok((Ok(handler_id),))
            },
        )?;

        // Add route implementation
        let servers_clone = self.servers.clone();
        let handlers_clone = self.handlers.clone();
        let next_route_id = self.next_route_id.clone();
        interface.func_wrap(
            "add-route",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id, path, method, handler_id): (u64, String, String, u64)|
                  -> Result<(Result<u64, String>,)> {
                let servers_clone = servers_clone.clone();
                let handlers_clone = handlers_clone.clone();
                let route_id = next_route_id.fetch_add(1, Ordering::SeqCst);

                // Validate path
                if !path.starts_with('/') {
                    return Ok((Err("Path must start with /".to_string()),));
                }

                // Validate method
                let method = method.to_uppercase();
                if !is_valid_method(&method) {
                    return Ok((Err(format!("Invalid HTTP method: {}", method)),));
                }

                // Clone values before moving to async context
                let path_clone = path.clone();
                let method_clone = method.clone();

                // Capture the current execution context for async operations
                let current_task = tokio::task::spawn(async move {
                    // Verify handler exists
                    let handlers = handlers_clone.read().await;
                    if !handlers.exists(handler_id) {
                        return Err(format!("Handler not found: {}", handler_id));
                    }

                    // Mark handler as HTTP request handler
                    handlers
                        .set_handler_type(handler_id, HandlerType::HttpRequest)
                        .await;

                    // Verify server exists and add route
                    let mut servers = servers_clone.write().await;
                    if let Some(server) = servers.get_mut(&server_id) {
                        match server
                            .add_route(
                                route_id,
                                path_clone.clone(),
                                method_clone.clone(),
                                handler_id,
                            )
                            .await
                        {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                let path_clone = path.clone();
                let method_clone = method.clone();

                match result {
                    Ok(_) => {
                        // Record event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/add-route".to_string(),
                            data: EventData::Http(HttpEventData::RouteAdd {
                                route_id,
                                server_id,
                                path: path_clone.clone(),
                                method: method_clone.clone(),
                                handler_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Added route {} for {} {} on server {}",
                                route_id, method_clone, path_clone, server_id
                            )),
                        });

                        Ok((Ok(route_id),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Remove route implementation
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "remove-route",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (route_id,): (u64,)|
                  -> Result<(Result<(), String>,)> {
                let servers_clone = servers_clone.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let mut servers = servers_clone.write().await;

                    // Find server with this route
                    let mut found = false;
                    let mut server_id = 0;

                    for (id, server) in servers.iter_mut() {
                        if server.has_route(route_id) {
                            found = true;
                            server_id = *id;
                            if let Err(e) = server.remove_route(route_id) {
                                return Err(e.to_string());
                            }
                            break;
                        }
                    }

                    if found {
                        Ok(server_id)
                    } else {
                        Err(format!("Route not found: {}", route_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                match result {
                    Ok(server_id) => {
                        // Record event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/remove-route".to_string(),
                            data: EventData::Http(HttpEventData::RouteRemove {
                                route_id,
                                server_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Removed route {} from server {}",
                                route_id, server_id
                            )),
                        });

                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Add middleware implementation
        let servers_clone = self.servers.clone();
        let handlers_clone = self.handlers.clone();
        let next_middleware_id = self.next_middleware_id.clone();
        interface.func_wrap(
            "add-middleware",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id, path, handler_id): (u64, String, u64)|
                  -> Result<(Result<u64, String>,)> {
                let servers_clone = servers_clone.clone();
                let handlers_clone = handlers_clone.clone();
                let middleware_id = next_middleware_id.fetch_add(1, Ordering::SeqCst);

                // Validate path
                if !path.starts_with('/') {
                    return Ok((Err("Path must start with /".to_string()),));
                }

                // Clone values before moving to async context
                let path_clone = path.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    // Verify handler exists
                    let handlers = handlers_clone.read().await;
                    if !handlers.exists(handler_id) {
                        return Err(format!("Handler not found: {}", handler_id));
                    }

                    // Mark handler as middleware handler
                    handlers
                        .set_handler_type(handler_id, HandlerType::Middleware)
                        .await;

                    // Verify server exists and add middleware
                    let mut servers = servers_clone.write().await;
                    if let Some(server) = servers.get_mut(&server_id) {
                        match server.add_middleware(middleware_id, path_clone.clone(), handler_id) {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                let path_clone = path.clone();

                match result {
                    Ok(_) => {
                        // Record event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/add-middleware".to_string(),
                            data: EventData::Http(HttpEventData::MiddlewareAdd {
                                middleware_id,
                                server_id,
                                path: path_clone.clone(),
                                handler_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Added middleware {} for path {} on server {}",
                                middleware_id, path_clone, server_id
                            )),
                        });

                        Ok((Ok(middleware_id),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Other implementation methods go here...

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
            
            // First stop all the servers
            {
                let servers = servers_ref.read().await;
                debug!("HTTP Framework shutting down {} servers", servers.len());
                
                // Get server IDs
                let server_ids: Vec<u64> = servers.keys().cloned().collect();
                
                // Create a vector to hold futures
                let mut futures = Vec::new();
                
                // Stop each server in parallel
                for server_id in server_ids {
                    let servers_clone = servers_ref.clone();
                    let fut = tokio::spawn(async move {
                        let mut servers = servers_clone.write().await;
                        if let Some(server) = servers.get_mut(&server_id) {
                            debug!("Stopping HTTP server {}", server_id);
                            if let Err(e) = server.stop().await {
                                warn!("Error stopping HTTP server {}: {}", server_id, e);
                            } else {
                                debug!("Successfully stopped HTTP server {}", server_id);
                            }
                        }
                    });
                    futures.push(fut);
                }
                
                // Wait for all servers to be stopped
                for fut in futures {
                    let _ = fut.await;
                }
            }
            
            // Then clean up the handles
            {
                let mut handles = server_handles_ref.write().await;
                let handle_ids: Vec<u64> = handles.keys().cloned().collect();
                
                for handle_id in handle_ids {
                    if let Some(handle) = handles.remove(&handle_id) {
                        if let Some(task) = handle.task {
                            if !task.is_finished() {
                                task.abort();
                                debug!("Aborted server task for server {}", handle_id);
                            }
                        }
                    }
                }
            }
            
            // Add a longer delay to ensure OS has time to release sockets
            debug!("Waiting for OS to release socket resources");
            tokio::time::sleep(Duration::from_millis(500)).await;
            
            debug!("HTTP Framework shutdown complete");
        });
        
        Ok(())
    }
}
