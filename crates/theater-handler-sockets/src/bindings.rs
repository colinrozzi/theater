//! Bindgen-generated bindings for WASI Sockets interfaces
//!
//! This module uses wasmtime's bindgen! macro to generate type-safe Host traits
//! from the WASI sockets WIT definitions.

use wasmtime::component::bindgen;

bindgen!({
    world: "sockets-handler-host",
    path: "wit",
    with: {
        // Map WASI I/O resources (we reuse from io handler)
        "wasi:io/error/error": theater_handler_io::IoError,
        "wasi:io/streams/input-stream": theater_handler_io::InputStream,
        "wasi:io/streams/output-stream": theater_handler_io::OutputStream,
        "wasi:io/poll/pollable": crate::poll::SocketsPollable,

        // Map WASI Sockets resources to our backing types
        "wasi:sockets/network/network": crate::types::Network,
        "wasi:sockets/tcp/tcp-socket": crate::types::TcpSocket,
        "wasi:sockets/udp/udp-socket": crate::types::UdpSocket,
        "wasi:sockets/udp/incoming-datagram-stream": crate::types::IncomingDatagramStream,
        "wasi:sockets/udp/outgoing-datagram-stream": crate::types::OutgoingDatagramStream,
        "wasi:sockets/ip-name-lookup/resolve-address-stream": crate::types::ResolveAddressStream,
    },
    async: true,
    trappable_imports: true,
});

// Re-export Host traits for network
pub use wasi::sockets::network::Host as NetworkHost;
pub use wasi::sockets::network::HostNetwork;
pub use wasi::sockets::instance_network::Host as InstanceNetworkHost;

// Re-export Host traits for TCP
pub use wasi::sockets::tcp::Host as TcpHost;
pub use wasi::sockets::tcp::HostTcpSocket;
pub use wasi::sockets::tcp_create_socket::Host as TcpCreateSocketHost;

// Re-export Host traits for UDP
pub use wasi::sockets::udp::Host as UdpHost;
pub use wasi::sockets::udp::HostUdpSocket;
pub use wasi::sockets::udp::HostIncomingDatagramStream;
pub use wasi::sockets::udp::HostOutgoingDatagramStream;
pub use wasi::sockets::udp_create_socket::Host as UdpCreateSocketHost;

// Re-export Host traits for DNS
pub use wasi::sockets::ip_name_lookup::Host as IpNameLookupHost;
pub use wasi::sockets::ip_name_lookup::HostResolveAddressStream;

// Re-export I/O Host traits (needed for stream operations)
pub use wasi::io::error::Host as ErrorHost;
pub use wasi::io::error::HostError;
pub use wasi::io::poll::Host as PollHost;
pub use wasi::io::poll::HostPollable;
pub use wasi::io::streams::Host as StreamsHost;
pub use wasi::io::streams::HostInputStream;
pub use wasi::io::streams::HostOutputStream;

// Re-export clocks Host traits (TCP uses duration)
pub use wasi::clocks::monotonic_clock::Host as MonotonicClockHost;

// Re-export types that will be useful
pub use wasi::sockets::network::{ErrorCode, IpAddress, IpAddressFamily, IpSocketAddress};
pub use wasi::sockets::tcp::ShutdownType;
pub use wasi::sockets::udp::{IncomingDatagram, OutgoingDatagram};
