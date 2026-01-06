//! # Theater WASI I/O Handler
//!
//! This handler provides WASI I/O interfaces:
//! - `wasi:io/error` - Error resource type
//! - `wasi:io/streams` - Input and output stream resources
//! - `wasi:io/poll` - Pollable resources for stream subscriptions
//!
//! It also provides WASI CLI interfaces:
//! - `wasi:cli/stdin`, `wasi:cli/stdout`, `wasi:cli/stderr`
//! - `wasi:cli/environment`, `wasi:cli/exit`
//! - `wasi:cli/terminal-*` interfaces
//!
//! ## Architecture
//!
//! Streams are backed by in-memory buffers and provide non-blocking I/O operations.
//! This enables integration with Theater's event system and WASI HTTP bodies.
//!
//! This handler uses wasmtime's bindgen to generate type-safe Host traits from
//! the WASI I/O and CLI WIT definitions.

pub mod streams;
pub mod error;
pub mod events;
pub mod poll;
pub mod bindings;
pub mod host_impl;

pub use streams::{InputStream, OutputStream, IoHandler};
pub use error::{IoError, StreamError};
pub use events::IoEventData;
pub use poll::IoHandlerPollable;

use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::wasm::{ActorComponent, ActorInstance};
use theater::actor::{handle::ActorHandle, ActorStore};
use theater::shutdown::ShutdownReceiver;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use tracing::{debug, info};

/// WASI I/O handler that provides streams and error handling
pub struct WasiIoHandler {
    // Handler state if needed
}

impl WasiIoHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl Handler for WasiIoHandler
{
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(Self::new())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async { Ok(()) })
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent, ctx: &mut HandlerContext) -> Result<()> {
        debug!("WasiIoHandler::setup_host_functions() starting");

        // Use bindgen-generated add_to_linker calls for all interfaces
        // This is much simpler than manual func_wrap and ensures type safety
        // Skip interfaces already satisfied by other handlers (e.g., sockets handler)

        // wasi:io/error interface
        if !ctx.is_satisfied("wasi:io/error@0.2.3") {
            info!("Setting up wasi:io/error interface");
            bindings::wasi::io::error::add_to_linker(
                &mut actor_component.linker,
                |state: &mut ActorStore| state,
            )?;
            ctx.mark_satisfied("wasi:io/error@0.2.3");
        } else {
            debug!("wasi:io/error@0.2.3 already satisfied, skipping");
        }

        // wasi:io/poll interface
        if !ctx.is_satisfied("wasi:io/poll@0.2.3") {
            info!("Setting up wasi:io/poll interface");
            bindings::wasi::io::poll::add_to_linker(
                &mut actor_component.linker,
                |state: &mut ActorStore| state,
            )?;
            ctx.mark_satisfied("wasi:io/poll@0.2.3");
        } else {
            debug!("wasi:io/poll@0.2.3 already satisfied, skipping");
        }

        // wasi:io/streams interface
        if !ctx.is_satisfied("wasi:io/streams@0.2.3") {
            info!("Setting up wasi:io/streams interface");
            bindings::wasi::io::streams::add_to_linker(
                &mut actor_component.linker,
                |state: &mut ActorStore| state,
            )?;
            ctx.mark_satisfied("wasi:io/streams@0.2.3");
        } else {
            debug!("wasi:io/streams@0.2.3 already satisfied, skipping");
        }

        // wasi:cli/stdin interface
        info!("Setting up wasi:cli/stdin interface");
        bindings::wasi::cli::stdin::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/stdout interface
        info!("Setting up wasi:cli/stdout interface");
        bindings::wasi::cli::stdout::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/stderr interface
        info!("Setting up wasi:cli/stderr interface");
        bindings::wasi::cli::stderr::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/environment interface
        info!("Setting up wasi:cli/environment interface");
        bindings::wasi::cli::environment::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/exit interface
        info!("Setting up wasi:cli/exit interface");
        bindings::wasi::cli::exit::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/terminal-input interface
        info!("Setting up wasi:cli/terminal-input interface");
        bindings::wasi::cli::terminal_input::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/terminal-output interface
        info!("Setting up wasi:cli/terminal-output interface");
        bindings::wasi::cli::terminal_output::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/terminal-stdin interface
        info!("Setting up wasi:cli/terminal-stdin interface");
        bindings::wasi::cli::terminal_stdin::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/terminal-stdout interface
        info!("Setting up wasi:cli/terminal-stdout interface");
        bindings::wasi::cli::terminal_stdout::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        // wasi:cli/terminal-stderr interface
        info!("Setting up wasi:cli/terminal-stderr interface");
        bindings::wasi::cli::terminal_stderr::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;

        info!("WasiIoHandler setup complete");
        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance,
    ) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "wasi-io"
    }

    fn imports(&self) -> Option<Vec<String>> {
        // These versions should match the WASI 0.2.3 specification
        Some(vec![
            "wasi:io/streams@0.2.3".to_string(),
            "wasi:io/error@0.2.3".to_string(),
            "wasi:io/poll@0.2.3".to_string(),
            "wasi:cli/stdin@0.2.3".to_string(),
            "wasi:cli/stdout@0.2.3".to_string(),
            "wasi:cli/stderr@0.2.3".to_string(),
            "wasi:cli/environment@0.2.3".to_string(),
            "wasi:cli/exit@0.2.3".to_string(),
            "wasi:cli/terminal-input@0.2.3".to_string(),
            "wasi:cli/terminal-output@0.2.3".to_string(),
            "wasi:cli/terminal-stdin@0.2.3".to_string(),
            "wasi:cli/terminal-stdout@0.2.3".to_string(),
            "wasi:cli/terminal-stderr@0.2.3".to_string(),
        ])
    }

    fn exports(&self) -> Option<Vec<String>> {
        None
    }
}

impl Default for WasiIoHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension type for storing CLI arguments in ActorStore
/// Set by the runtime when starting an actor with arguments
#[derive(Clone, Debug)]
pub struct CliArguments(pub Vec<String>);

/// Extension type for storing initial working directory in ActorStore
/// Can be set by filesystem handler or other handlers
#[derive(Clone, Debug)]
pub struct InitialCwd(pub Option<String>);
