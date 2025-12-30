//! # Theater WASI Sockets Handler
//!
//! This handler provides WASI sockets interfaces for TCP/UDP networking:
//! - `wasi:sockets/network` - Network resource type
//! - `wasi:sockets/instance-network` - Default network instance
//! - `wasi:sockets/tcp` - TCP socket operations
//! - `wasi:sockets/tcp-create-socket` - TCP socket creation
//! - `wasi:sockets/udp` - UDP socket operations
//! - `wasi:sockets/udp-create-socket` - UDP socket creation
//! - `wasi:sockets/ip-name-lookup` - DNS resolution
//!
//! ## Architecture
//!
//! Socket resources are backed by tokio async sockets. All operations are
//! recorded in the event chain for replay and verification purposes.
//!
//! This handler uses wasmtime's bindgen to generate type-safe Host traits from
//! the WASI sockets WIT definitions.

pub mod bindings;
pub mod events;
pub mod host_impl;
pub mod poll;
pub mod types;

pub use events::SocketsEventData;
pub use poll::SocketsPollable;
pub use types::{
    Network, TcpSocket, TcpSocketState, UdpSocket, UdpSocketState,
    IncomingDatagramStream, OutgoingDatagramStream, ResolveAddressStream,
    IpAddressFamily,
};

use theater::handler::{Handler, SharedActorInstance};
use theater::events::EventPayload;
use theater::wasm::{ActorComponent, ActorInstance};
use theater::actor::{handle::ActorHandle, ActorStore};
use theater::shutdown::ShutdownReceiver;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use tracing::{debug, info};

/// WASI Sockets handler that provides TCP/UDP networking
pub struct WasiSocketsHandler {
    // Handler state if needed
}

impl WasiSocketsHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl<E> Handler<E> for WasiSocketsHandler
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    fn create_instance(&self) -> Box<dyn Handler<E>> {
        Box::new(Self::new())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance<E>,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async { Ok(()) })
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent<E>) -> Result<()> {
        debug!("WasiSocketsHandler::setup_host_functions() starting");

        // wasi:io/error interface (sockets use this)
        info!("Setting up wasi:io/error interface for sockets");
        bindings::wasi::io::error::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:io/poll interface (sockets use pollables)
        info!("Setting up wasi:io/poll interface for sockets");
        bindings::wasi::io::poll::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:io/streams interface (TCP connections use streams)
        info!("Setting up wasi:io/streams interface for sockets");
        bindings::wasi::io::streams::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:clocks/monotonic-clock interface (TCP keepalive uses duration)
        info!("Setting up wasi:clocks/monotonic-clock interface for sockets");
        bindings::wasi::clocks::monotonic_clock::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:sockets/network interface
        info!("Setting up wasi:sockets/network interface");
        bindings::wasi::sockets::network::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:sockets/instance-network interface
        info!("Setting up wasi:sockets/instance-network interface");
        bindings::wasi::sockets::instance_network::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:sockets/tcp interface
        info!("Setting up wasi:sockets/tcp interface");
        bindings::wasi::sockets::tcp::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:sockets/tcp-create-socket interface
        info!("Setting up wasi:sockets/tcp-create-socket interface");
        bindings::wasi::sockets::tcp_create_socket::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:sockets/udp interface
        info!("Setting up wasi:sockets/udp interface");
        bindings::wasi::sockets::udp::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:sockets/udp-create-socket interface
        info!("Setting up wasi:sockets/udp-create-socket interface");
        bindings::wasi::sockets::udp_create_socket::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        // wasi:sockets/ip-name-lookup interface
        info!("Setting up wasi:sockets/ip-name-lookup interface");
        bindings::wasi::sockets::ip_name_lookup::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;

        info!("WasiSocketsHandler setup complete");
        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance<E>,
    ) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "wasi-sockets"
    }

    fn imports(&self) -> Option<String> {
        // Support both 0.2.0 and 0.2.3 versions (ABI compatible)
        Some("wasi:sockets/network@0.2.0,wasi:sockets/instance-network@0.2.0,wasi:sockets/tcp@0.2.0,wasi:sockets/tcp-create-socket@0.2.0,wasi:sockets/udp@0.2.0,wasi:sockets/udp-create-socket@0.2.0,wasi:sockets/ip-name-lookup@0.2.0,wasi:sockets/network@0.2.3,wasi:sockets/instance-network@0.2.3,wasi:sockets/tcp@0.2.3,wasi:sockets/tcp-create-socket@0.2.3,wasi:sockets/udp@0.2.3,wasi:sockets/udp-create-socket@0.2.3,wasi:sockets/ip-name-lookup@0.2.3".to_string())
    }

    fn exports(&self) -> Option<String> {
        None
    }
}

impl Default for WasiSocketsHandler {
    fn default() -> Self {
        Self::new()
    }
}
