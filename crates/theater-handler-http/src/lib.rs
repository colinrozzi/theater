//! # Theater WASI HTTP Handler
//!
//! This handler provides WASI HTTP interfaces:
//! - `wasi:http/incoming-handler` - HTTP server capability
//! - `wasi:http/outgoing-handler` - HTTP client capability
//! - `wasi:http/types` - Common HTTP types (headers, methods, status codes)
//!
//! ## Architecture
//!
//! - **Incoming Handler**: Uses hyper to handle HTTP requests, providing request
//!   resources to WASM components and receiving response resources back.
//! - **Outgoing Handler**: Uses reqwest to make HTTP requests, allowing WASM
//!   components to make outgoing HTTP calls.
//! - **Streams Integration**: HTTP bodies are backed by wasi:io/streams for
//!   true streaming support with constant memory usage.

pub mod types;
pub mod incoming;
pub mod outgoing;
pub mod events;
pub mod bindings;
pub mod host_impl;
mod server;

pub use types::*;
pub use events::HttpEventData;

use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::wasm::{ActorComponent, ActorInstance};
use theater::actor::handle::ActorHandle;
use theater::shutdown::ShutdownReceiver;
use theater::events::theater_runtime::TheaterRuntimeEventData;
use theater::events::wasm::WasmEventData;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use tracing::{debug, info, error};

/// Configuration for the WASI HTTP incoming server
#[derive(Debug, Clone)]
pub struct WasiHttpConfig {
    /// Port to listen on for incoming HTTP requests
    /// If None, no incoming handler server will be started
    pub port: Option<u16>,
    /// Host to bind to (default: 127.0.0.1)
    pub host: String,
}

impl Default for WasiHttpConfig {
    fn default() -> Self {
        Self {
            port: None,
            host: "127.0.0.1".to_string(),
        }
    }
}

/// WASI HTTP handler that provides both incoming and outgoing HTTP
pub struct WasiHttpHandler {
    config: WasiHttpConfig,
}

impl WasiHttpHandler {
    pub fn new() -> Self {
        Self {
            config: WasiHttpConfig::default(),
        }
    }

    pub fn with_config(config: WasiHttpConfig) -> Self {
        Self { config }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.config.port = Some(port);
        self
    }

    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.config.host = host.into();
        self
    }
}

impl Handler for WasiHttpHandler
{
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(Self {
            config: self.config.clone(),
        })
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let port = self.config.port;
        let host = self.config.host.clone();

        Box::pin(async move {
            // Only start the server if a port is configured
            let Some(port) = port else {
                debug!("No port configured for WASI HTTP incoming handler, server not started");
                // Wait for shutdown
                let _ = shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            };

            info!("Starting WASI HTTP incoming handler server on {}:{}", host, port);

            // Start the HTTP server
            match server::start_incoming_server(
                actor_instance,
                &host,
                port,
                shutdown_receiver,
            ).await {
                Ok(()) => {
                    info!("WASI HTTP server shut down gracefully");
                    Ok(())
                }
                Err(e) => {
                    error!("WASI HTTP server error: {:?}", e);
                    Err(e)
                }
            }
        })
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent, _ctx: &mut HandlerContext) -> Result<()> {
        use crate::bindings;
        use theater::actor::ActorStore;

        info!("Setting up WASI HTTP interfaces using bindgen-generated add_to_linker");

        // Add wasi:io/error interface (HTTP depends on this)
        bindings::wasi::io::error::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        debug!("wasi:io/error interface added");

        // Add wasi:io/streams interface (HTTP depends on this)
        bindings::wasi::io::streams::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        debug!("wasi:io/streams interface added");

        // Add wasi:http/types interface (all HTTP types and resources)
        bindings::wasi::http::types::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        debug!("wasi:http/types interface added");

        // Add wasi:http/outgoing-handler interface (HTTP client)
        bindings::wasi::http::outgoing_handler::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        debug!("wasi:http/outgoing-handler interface added");

        // Note: wasi:http/incoming-handler is an EXPORT (component implements it)
        // We don't add it to the linker - instead we call it when handling requests

        info!("WasiHttpHandler setup complete (using bindgen traits)");
        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance,
    ) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "wasi-http"
    }

    fn imports(&self) -> Option<Vec<String>> {
        // Component imports these interfaces (we provide them to the component)
        // HTTP handler also provides IO interfaces since HTTP depends on them
        Some(vec![
            "wasi:io/error@0.2.3".to_string(),
            "wasi:io/streams@0.2.3".to_string(),
            "wasi:http/types@0.2.0".to_string(),
            "wasi:http/outgoing-handler@0.2.0".to_string(),
        ])
    }

    fn exports(&self) -> Option<Vec<String>> {
        // Component exports this interface (we call it to handle incoming requests)
        Some(vec!["wasi:http/incoming-handler@0.2.0".to_string()])
    }
}

impl Default for WasiHttpHandler {
    fn default() -> Self {
        Self::new()
    }
}
