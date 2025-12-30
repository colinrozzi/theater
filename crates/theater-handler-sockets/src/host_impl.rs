//! Host trait implementations for WASI Sockets interfaces
//!
//! These implementations provide networking capabilities to actors while
//! recording all operations in the event chain for replay.

use crate::bindings::{
    NetworkHost, HostNetwork, InstanceNetworkHost,
    TcpHost, HostTcpSocket, TcpCreateSocketHost,
    UdpHost, HostUdpSocket, HostIncomingDatagramStream, HostOutgoingDatagramStream,
    UdpCreateSocketHost, IpNameLookupHost, HostResolveAddressStream,
    ErrorHost, HostError, PollHost, HostPollable,
    StreamsHost, HostInputStream, HostOutputStream,
    MonotonicClockHost,
    ErrorCode, IpSocketAddress, ShutdownType,
    IncomingDatagram, OutgoingDatagram,
};
use crate::events::SocketsEventData;
use crate::poll::SocketsPollable;
use crate::types::{
    Network, TcpSocket, TcpSocketState, UdpSocket, UdpSocketState,
    IncomingDatagramStream, OutgoingDatagramStream, ResolveAddressStream,
    IpAddressFamily, wit_socket_addr_to_std, std_socket_addr_to_wit, std_ip_to_wit,
};
use anyhow::Result;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream, UdpSocket as TokioUdpSocket};
use wasmtime::component::Resource;
use theater::actor::ActorStore;
use theater::events::EventPayload;
use theater_handler_io::{IoError, InputStream, OutputStream};
use tracing::{debug, warn};

// =============================================================================
// wasi:io/error Host implementation (delegate to io handler types)
// =============================================================================

impl<E> ErrorHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
}

impl<E> HostError for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn to_debug_string(&mut self, error: Resource<IoError>) -> Result<String> {
        let table = self.resource_table.lock().unwrap();
        let io_error: &IoError = table.get(&error)?;
        Ok(io_error.to_debug_string())
    }

    async fn drop(&mut self, error: Resource<IoError>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(error);
        Ok(())
    }
}

// =============================================================================
// wasi:io/poll Host implementation
// =============================================================================

impl<E> PollHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn poll(&mut self, pollables: Vec<Resource<SocketsPollable>>) -> Result<Vec<u32>> {
        let mut ready_indices = Vec::new();
        for (idx, pollable_handle) in pollables.iter().enumerate() {
            let is_ready = {
                let table = self.resource_table.lock().unwrap();
                if let Ok(pollable) = table.get(pollable_handle) {
                    pollable.is_ready()
                } else {
                    false
                }
            };
            if is_ready {
                ready_indices.push(idx as u32);
            }
        }
        Ok(ready_indices)
    }
}

impl<E> HostPollable for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn ready(&mut self, pollable: Resource<SocketsPollable>) -> Result<bool> {
        let table = self.resource_table.lock().unwrap();
        if let Ok(p) = table.get(&pollable) {
            Ok(p.is_ready())
        } else {
            Ok(false)
        }
    }

    async fn block(&mut self, pollable: Resource<SocketsPollable>) -> Result<()> {
        let p = {
            let table = self.resource_table.lock().unwrap();
            table.get(&pollable)?.clone()
        };
        p.block().await;
        Ok(())
    }

    async fn drop(&mut self, pollable: Resource<SocketsPollable>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(pollable);
        Ok(())
    }
}

// =============================================================================
// wasi:io/streams Host implementation (delegate to io handler)
// =============================================================================

impl<E> StreamsHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
}

impl<E> HostInputStream for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn read(&mut self, stream: Resource<InputStream>, len: u64) -> Result<Result<Vec<u8>, crate::bindings::wasi::io::streams::StreamError>> {
        let table = self.resource_table.lock().unwrap();
        let input_stream: &InputStream = table.get(&stream)?;
        match input_stream.read(len) {
            Ok(data) => Ok(Ok(data)),
            Err(_) => Ok(Err(crate::bindings::wasi::io::streams::StreamError::Closed)),
        }
    }

    async fn blocking_read(&mut self, stream: Resource<InputStream>, len: u64) -> Result<Result<Vec<u8>, crate::bindings::wasi::io::streams::StreamError>> {
        self.read(stream, len).await
    }

    async fn skip(&mut self, stream: Resource<InputStream>, len: u64) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        let table = self.resource_table.lock().unwrap();
        let input_stream: &InputStream = table.get(&stream)?;
        match input_stream.skip(len) {
            Ok(skipped) => Ok(Ok(skipped)),
            Err(_) => Ok(Err(crate::bindings::wasi::io::streams::StreamError::Closed)),
        }
    }

    async fn blocking_skip(&mut self, stream: Resource<InputStream>, len: u64) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        self.skip(stream, len).await
    }

    async fn subscribe(&mut self, _stream: Resource<InputStream>) -> Result<Resource<SocketsPollable>> {
        let pollable = SocketsPollable::ready();
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn drop(&mut self, stream: Resource<InputStream>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(stream);
        Ok(())
    }
}

impl<E> HostOutputStream for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn check_write(&mut self, stream: Resource<OutputStream>) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        let table = self.resource_table.lock().unwrap();
        let output_stream: &OutputStream = table.get(&stream)?;
        match output_stream.check_write() {
            Ok(available) => Ok(Ok(available)),
            Err(_) => Ok(Err(crate::bindings::wasi::io::streams::StreamError::Closed)),
        }
    }

    async fn write(&mut self, stream: Resource<OutputStream>, contents: Vec<u8>) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        let table = self.resource_table.lock().unwrap();
        let output_stream: &OutputStream = table.get(&stream)?;
        match output_stream.write(&contents) {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(crate::bindings::wasi::io::streams::StreamError::Closed)),
        }
    }

    async fn blocking_write_and_flush(&mut self, stream: Resource<OutputStream>, contents: Vec<u8>) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        self.write(stream, contents).await
    }

    async fn flush(&mut self, stream: Resource<OutputStream>) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        let table = self.resource_table.lock().unwrap();
        let output_stream: &OutputStream = table.get(&stream)?;
        match output_stream.flush() {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(crate::bindings::wasi::io::streams::StreamError::Closed)),
        }
    }

    async fn blocking_flush(&mut self, stream: Resource<OutputStream>) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        self.flush(stream).await
    }

    async fn subscribe(&mut self, _stream: Resource<OutputStream>) -> Result<Resource<SocketsPollable>> {
        let pollable = SocketsPollable::ready();
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn write_zeroes(&mut self, stream: Resource<OutputStream>, len: u64) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        let table = self.resource_table.lock().unwrap();
        let output_stream: &OutputStream = table.get(&stream)?;
        match output_stream.write_zeroes(len) {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(crate::bindings::wasi::io::streams::StreamError::Closed)),
        }
    }

    async fn blocking_write_zeroes_and_flush(&mut self, stream: Resource<OutputStream>, len: u64) -> Result<Result<(), crate::bindings::wasi::io::streams::StreamError>> {
        self.write_zeroes(stream, len).await
    }

    async fn splice(&mut self, _dst: Resource<OutputStream>, _src: Resource<InputStream>, _len: u64) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        // Splice not fully supported for socket streams
        Ok(Err(crate::bindings::wasi::io::streams::StreamError::Closed))
    }

    async fn blocking_splice(&mut self, dst: Resource<OutputStream>, src: Resource<InputStream>, len: u64) -> Result<Result<u64, crate::bindings::wasi::io::streams::StreamError>> {
        self.splice(dst, src, len).await
    }

    async fn drop(&mut self, stream: Resource<OutputStream>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(stream);
        Ok(())
    }
}

// =============================================================================
// wasi:clocks/monotonic-clock Host implementation
// =============================================================================

impl<E> MonotonicClockHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn now(&mut self) -> Result<u64> {
        Ok(std::time::Instant::now().elapsed().as_nanos() as u64)
    }

    async fn resolution(&mut self) -> Result<u64> {
        Ok(1) // 1 nanosecond resolution
    }

    async fn subscribe_instant(&mut self, _when: u64) -> Result<Resource<SocketsPollable>> {
        // Return a pollable that's always ready for now
        let pollable = SocketsPollable::ready();
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn subscribe_duration(&mut self, _when: u64) -> Result<Resource<SocketsPollable>> {
        let pollable = SocketsPollable::ready();
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }
}

// =============================================================================
// wasi:sockets/network Host implementation
// =============================================================================

impl<E> NetworkHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
}

impl<E> HostNetwork for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn drop(&mut self, network: Resource<Network>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(network);
        Ok(())
    }
}

// =============================================================================
// wasi:sockets/instance-network Host implementation
// =============================================================================

impl<E> InstanceNetworkHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn instance_network(&mut self) -> Result<Resource<Network>> {
        debug!("wasi:sockets/instance-network instance-network");

        self.record_handler_event(
            "wasi:sockets/instance-network".to_string(),
            SocketsEventData::InstanceNetworkCall,
            Some("Getting default network instance".to_string()),
        );

        let network = Network::default_network();
        let resource = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(network)?
        };

        let network_id = resource.rep();
        self.record_handler_event(
            "wasi:sockets/instance-network".to_string(),
            SocketsEventData::InstanceNetworkResult { network_id },
            Some(format!("Created network instance {}", network_id)),
        );

        Ok(resource)
    }
}

// =============================================================================
// wasi:sockets/tcp-create-socket Host implementation
// =============================================================================

impl<E> TcpCreateSocketHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn create_tcp_socket(
        &mut self,
        address_family: crate::bindings::wasi::sockets::network::IpAddressFamily,
    ) -> Result<Result<Resource<TcpSocket>, ErrorCode>> {
        let family = match address_family {
            crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv4 => IpAddressFamily::Ipv4,
            crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv6 => IpAddressFamily::Ipv6,
        };

        debug!("wasi:sockets/tcp-create-socket create-tcp-socket: {:?}", family);

        self.record_handler_event(
            "wasi:sockets/tcp-create-socket".to_string(),
            SocketsEventData::CreateTcpSocketCall { family: format!("{:?}", family) },
            Some(format!("Creating TCP socket for {:?}", family)),
        );

        let socket = TcpSocket::new(family);
        let resource = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(socket)?
        };

        let socket_id = resource.rep();
        self.record_handler_event(
            "wasi:sockets/tcp-create-socket".to_string(),
            SocketsEventData::CreateTcpSocketResult { socket_id, success: true },
            Some(format!("Created TCP socket {}", socket_id)),
        );

        Ok(Ok(resource))
    }
}

// =============================================================================
// wasi:sockets/tcp Host implementation
// =============================================================================

impl<E> TcpHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
}

impl<E> HostTcpSocket for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn start_bind(
        &mut self,
        socket: Resource<TcpSocket>,
        _network: Resource<Network>,
        local_address: IpSocketAddress,
    ) -> Result<Result<(), ErrorCode>> {
        let addr = wit_socket_addr_to_std(&local_address);
        debug!("wasi:sockets/tcp start-bind: {}", addr);

        self.record_handler_event(
            "wasi:sockets/tcp/start-bind".to_string(),
            SocketsEventData::TcpStartBindCall { address: addr.to_string() },
            Some(format!("TCP start-bind to {}", addr)),
        );

        // Update socket state to bind-in-progress
        {
            let mut table = self.resource_table.lock().unwrap();
            let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;

            match &tcp_socket.state {
                TcpSocketState::Unbound => {
                    tcp_socket.state = TcpSocketState::BindInProgress { local_address: addr };
                }
                _ => {
                    return Ok(Err(ErrorCode::InvalidState));
                }
            }
        }

        self.record_handler_event(
            "wasi:sockets/tcp/start-bind".to_string(),
            SocketsEventData::TcpStartBindResult { success: true },
            Some("TCP start-bind initiated".to_string()),
        );

        Ok(Ok(()))
    }

    async fn finish_bind(&mut self, socket: Resource<TcpSocket>) -> Result<Result<(), ErrorCode>> {
        debug!("wasi:sockets/tcp finish-bind");

        self.record_handler_event(
            "wasi:sockets/tcp/finish-bind".to_string(),
            SocketsEventData::TcpFinishBindCall,
            Some("TCP finish-bind".to_string()),
        );

        // Actually perform the bind and update state
        let result = {
            let mut table = self.resource_table.lock().unwrap();
            let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;

            match &tcp_socket.state {
                TcpSocketState::BindInProgress { local_address } => {
                    let addr = *local_address;
                    tcp_socket.state = TcpSocketState::Bound { local_address: addr };
                    Ok(())
                }
                _ => Err(ErrorCode::NotInProgress),
            }
        };

        let success = result.is_ok();
        self.record_handler_event(
            "wasi:sockets/tcp/finish-bind".to_string(),
            SocketsEventData::TcpFinishBindResult { success },
            Some(format!("TCP finish-bind: success={}", success)),
        );

        Ok(result)
    }

    async fn start_connect(
        &mut self,
        socket: Resource<TcpSocket>,
        _network: Resource<Network>,
        remote_address: IpSocketAddress,
    ) -> Result<Result<(), ErrorCode>> {
        let addr = wit_socket_addr_to_std(&remote_address);
        debug!("wasi:sockets/tcp start-connect: {}", addr);

        self.record_handler_event(
            "wasi:sockets/tcp/start-connect".to_string(),
            SocketsEventData::TcpStartConnectCall { address: addr.to_string() },
            Some(format!("TCP start-connect to {}", addr)),
        );

        // Update socket state
        {
            let mut table = self.resource_table.lock().unwrap();
            let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;

            let local_addr = tcp_socket.local_address();
            match &tcp_socket.state {
                TcpSocketState::Unbound | TcpSocketState::Bound { .. } => {
                    tcp_socket.state = TcpSocketState::ConnectInProgress {
                        local_address: local_addr,
                        remote_address: addr,
                    };
                }
                _ => {
                    return Ok(Err(ErrorCode::InvalidState));
                }
            }
        }

        self.record_handler_event(
            "wasi:sockets/tcp/start-connect".to_string(),
            SocketsEventData::TcpStartConnectResult { success: true },
            Some("TCP start-connect initiated".to_string()),
        );

        Ok(Ok(()))
    }

    async fn finish_connect(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> Result<Result<(Resource<InputStream>, Resource<OutputStream>), ErrorCode>> {
        debug!("wasi:sockets/tcp finish-connect");

        self.record_handler_event(
            "wasi:sockets/tcp/finish-connect".to_string(),
            SocketsEventData::TcpFinishConnectCall,
            Some("TCP finish-connect".to_string()),
        );

        // Get the remote address from the socket state
        let remote_addr = {
            let table = self.resource_table.lock().unwrap();
            let tcp_socket: &TcpSocket = table.get(&socket)?;
            match &tcp_socket.state {
                TcpSocketState::ConnectInProgress { remote_address, .. } => *remote_address,
                _ => return Ok(Err(ErrorCode::NotInProgress)),
            }
        };

        // Attempt the actual connection
        match TcpStream::connect(remote_addr).await {
            Ok(stream) => {
                let local_addr = stream.local_addr().unwrap_or(remote_addr);

                // Update socket state
                {
                    let mut table = self.resource_table.lock().unwrap();
                    let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
                    tcp_socket.state = TcpSocketState::Connected {
                        stream,
                        local_address: local_addr,
                        remote_address: remote_addr,
                    };
                }

                // Create input/output streams
                // For now, create empty streams - a full implementation would wrap the TcpStream
                let input_stream = InputStream::from_bytes(Vec::new());
                let output_stream = OutputStream::new();

                let (input_res, output_res) = {
                    let mut table = self.resource_table.lock().unwrap();
                    (table.push(input_stream)?, table.push(output_stream)?)
                };

                self.record_handler_event(
                    "wasi:sockets/tcp/finish-connect".to_string(),
                    SocketsEventData::TcpFinishConnectResult { success: true },
                    Some("TCP connection established".to_string()),
                );

                Ok(Ok((input_res, output_res)))
            }
            Err(e) => {
                warn!("TCP connect failed: {}", e);

                // Update socket to closed state
                {
                    let mut table = self.resource_table.lock().unwrap();
                    let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
                    tcp_socket.state = TcpSocketState::Closed;
                }

                self.record_handler_event(
                    "wasi:sockets/tcp/finish-connect".to_string(),
                    SocketsEventData::TcpFinishConnectResult { success: false },
                    Some(format!("TCP connect failed: {}", e)),
                );

                Ok(Err(ErrorCode::ConnectionRefused))
            }
        }
    }

    async fn start_listen(&mut self, socket: Resource<TcpSocket>) -> Result<Result<(), ErrorCode>> {
        debug!("wasi:sockets/tcp start-listen");

        self.record_handler_event(
            "wasi:sockets/tcp/start-listen".to_string(),
            SocketsEventData::TcpStartListenCall,
            Some("TCP start-listen".to_string()),
        );

        {
            let mut table = self.resource_table.lock().unwrap();
            let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;

            match &tcp_socket.state {
                TcpSocketState::Bound { local_address } => {
                    let addr = *local_address;
                    tcp_socket.state = TcpSocketState::ListenInProgress { local_address: addr };
                }
                _ => {
                    return Ok(Err(ErrorCode::InvalidState));
                }
            }
        }

        self.record_handler_event(
            "wasi:sockets/tcp/start-listen".to_string(),
            SocketsEventData::TcpStartListenResult { success: true },
            Some("TCP start-listen initiated".to_string()),
        );

        Ok(Ok(()))
    }

    async fn finish_listen(&mut self, socket: Resource<TcpSocket>) -> Result<Result<(), ErrorCode>> {
        debug!("wasi:sockets/tcp finish-listen");

        self.record_handler_event(
            "wasi:sockets/tcp/finish-listen".to_string(),
            SocketsEventData::TcpFinishListenCall,
            Some("TCP finish-listen".to_string()),
        );

        // Get the local address
        let local_addr = {
            let table = self.resource_table.lock().unwrap();
            let tcp_socket: &TcpSocket = table.get(&socket)?;
            match &tcp_socket.state {
                TcpSocketState::ListenInProgress { local_address } => *local_address,
                _ => return Ok(Err(ErrorCode::NotInProgress)),
            }
        };

        // Actually start listening
        match TcpListener::bind(local_addr).await {
            Ok(listener) => {
                let actual_addr = listener.local_addr().unwrap_or(local_addr);

                {
                    let mut table = self.resource_table.lock().unwrap();
                    let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
                    tcp_socket.state = TcpSocketState::Listening {
                        listener,
                        local_address: actual_addr,
                    };
                }

                self.record_handler_event(
                    "wasi:sockets/tcp/finish-listen".to_string(),
                    SocketsEventData::TcpFinishListenResult { success: true },
                    Some(format!("TCP listening on {}", actual_addr)),
                );

                Ok(Ok(()))
            }
            Err(e) => {
                warn!("TCP listen failed: {}", e);

                self.record_handler_event(
                    "wasi:sockets/tcp/finish-listen".to_string(),
                    SocketsEventData::TcpFinishListenResult { success: false },
                    Some(format!("TCP listen failed: {}", e)),
                );

                Ok(Err(ErrorCode::AddressInUse))
            }
        }
    }

    async fn accept(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> Result<Result<(Resource<TcpSocket>, Resource<InputStream>, Resource<OutputStream>), ErrorCode>> {
        debug!("wasi:sockets/tcp accept");

        self.record_handler_event(
            "wasi:sockets/tcp/accept".to_string(),
            SocketsEventData::TcpAcceptCall,
            Some("TCP accept".to_string()),
        );

        // Check if socket is in listening state
        {
            let table = self.resource_table.lock().unwrap();
            let tcp_socket: &TcpSocket = table.get(&socket)?;
            if !matches!(tcp_socket.state, TcpSocketState::Listening { .. }) {
                return Ok(Err(ErrorCode::InvalidState));
            }
        }

        // For now, return would-block since async accept integration requires more work
        self.record_handler_event(
            "wasi:sockets/tcp/accept".to_string(),
            SocketsEventData::TcpAcceptResult { client_socket_id: 0, success: false },
            Some("TCP accept: would-block".to_string()),
        );

        Ok(Err(ErrorCode::WouldBlock))
    }

    async fn local_address(&mut self, socket: Resource<TcpSocket>) -> Result<Result<IpSocketAddress, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;

        match tcp_socket.local_address() {
            Some(addr) => Ok(Ok(std_socket_addr_to_wit(addr))),
            None => Ok(Err(ErrorCode::InvalidState)),
        }
    }

    async fn remote_address(&mut self, socket: Resource<TcpSocket>) -> Result<Result<IpSocketAddress, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;

        match tcp_socket.remote_address() {
            Some(addr) => Ok(Ok(std_socket_addr_to_wit(addr))),
            None => Ok(Err(ErrorCode::InvalidState)),
        }
    }

    async fn is_listening(&mut self, socket: Resource<TcpSocket>) -> Result<bool> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(tcp_socket.is_listening())
    }

    async fn address_family(&mut self, socket: Resource<TcpSocket>) -> Result<crate::bindings::wasi::sockets::network::IpAddressFamily> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(match tcp_socket.family {
            IpAddressFamily::Ipv4 => crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv4,
            IpAddressFamily::Ipv6 => crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv6,
        })
    }

    async fn set_listen_backlog_size(&mut self, socket: Resource<TcpSocket>, value: u64) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.listen_backlog_size = value;
        Ok(Ok(()))
    }

    async fn keep_alive_enabled(&mut self, socket: Resource<TcpSocket>) -> Result<Result<bool, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(Ok(tcp_socket.options.keep_alive_enabled))
    }

    async fn set_keep_alive_enabled(&mut self, socket: Resource<TcpSocket>, value: bool) -> Result<Result<(), ErrorCode>> {
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.keep_alive_enabled = value;
        Ok(Ok(()))
    }

    async fn keep_alive_idle_time(&mut self, socket: Resource<TcpSocket>) -> Result<Result<u64, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(Ok(tcp_socket.options.keep_alive_idle_time))
    }

    async fn set_keep_alive_idle_time(&mut self, socket: Resource<TcpSocket>, value: u64) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.keep_alive_idle_time = value;
        Ok(Ok(()))
    }

    async fn keep_alive_interval(&mut self, socket: Resource<TcpSocket>) -> Result<Result<u64, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(Ok(tcp_socket.options.keep_alive_interval))
    }

    async fn set_keep_alive_interval(&mut self, socket: Resource<TcpSocket>, value: u64) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.keep_alive_interval = value;
        Ok(Ok(()))
    }

    async fn keep_alive_count(&mut self, socket: Resource<TcpSocket>) -> Result<Result<u32, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(Ok(tcp_socket.options.keep_alive_count))
    }

    async fn set_keep_alive_count(&mut self, socket: Resource<TcpSocket>, value: u32) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.keep_alive_count = value;
        Ok(Ok(()))
    }

    async fn hop_limit(&mut self, socket: Resource<TcpSocket>) -> Result<Result<u8, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(Ok(tcp_socket.options.hop_limit))
    }

    async fn set_hop_limit(&mut self, socket: Resource<TcpSocket>, value: u8) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.hop_limit = value;
        Ok(Ok(()))
    }

    async fn receive_buffer_size(&mut self, socket: Resource<TcpSocket>) -> Result<Result<u64, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(Ok(tcp_socket.options.receive_buffer_size))
    }

    async fn set_receive_buffer_size(&mut self, socket: Resource<TcpSocket>, value: u64) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.receive_buffer_size = value;
        Ok(Ok(()))
    }

    async fn send_buffer_size(&mut self, socket: Resource<TcpSocket>) -> Result<Result<u64, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let tcp_socket: &TcpSocket = table.get(&socket)?;
        Ok(Ok(tcp_socket.options.send_buffer_size))
    }

    async fn set_send_buffer_size(&mut self, socket: Resource<TcpSocket>, value: u64) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let tcp_socket: &mut TcpSocket = table.get_mut(&socket)?;
        tcp_socket.options.send_buffer_size = value;
        Ok(Ok(()))
    }

    async fn subscribe(&mut self, socket: Resource<TcpSocket>) -> Result<Resource<SocketsPollable>> {
        let socket_id = socket.rep();
        let pollable = SocketsPollable::for_tcp_socket(socket_id);
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn shutdown(&mut self, _socket: Resource<TcpSocket>, shutdown_type: ShutdownType) -> Result<Result<(), ErrorCode>> {
        debug!("wasi:sockets/tcp shutdown: {:?}", shutdown_type);

        self.record_handler_event(
            "wasi:sockets/tcp/shutdown".to_string(),
            SocketsEventData::TcpShutdownCall { shutdown_type: format!("{:?}", shutdown_type) },
            Some(format!("TCP shutdown: {:?}", shutdown_type)),
        );

        // For now, just acknowledge the shutdown
        self.record_handler_event(
            "wasi:sockets/tcp/shutdown".to_string(),
            SocketsEventData::TcpShutdownResult { success: true },
            Some("TCP shutdown completed".to_string()),
        );

        Ok(Ok(()))
    }

    async fn drop(&mut self, socket: Resource<TcpSocket>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(socket);
        Ok(())
    }
}

// =============================================================================
// wasi:sockets/udp-create-socket Host implementation
// =============================================================================

impl<E> UdpCreateSocketHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn create_udp_socket(
        &mut self,
        address_family: crate::bindings::wasi::sockets::network::IpAddressFamily,
    ) -> Result<Result<Resource<UdpSocket>, ErrorCode>> {
        let family = match address_family {
            crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv4 => IpAddressFamily::Ipv4,
            crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv6 => IpAddressFamily::Ipv6,
        };

        debug!("wasi:sockets/udp-create-socket create-udp-socket: {:?}", family);

        self.record_handler_event(
            "wasi:sockets/udp-create-socket".to_string(),
            SocketsEventData::CreateUdpSocketCall { family: format!("{:?}", family) },
            Some(format!("Creating UDP socket for {:?}", family)),
        );

        let socket = UdpSocket::new(family);
        let resource = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(socket)?
        };

        let socket_id = resource.rep();
        self.record_handler_event(
            "wasi:sockets/udp-create-socket".to_string(),
            SocketsEventData::CreateUdpSocketResult { socket_id, success: true },
            Some(format!("Created UDP socket {}", socket_id)),
        );

        Ok(Ok(resource))
    }
}

// =============================================================================
// wasi:sockets/udp Host implementation
// =============================================================================

impl<E> UdpHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
}

impl<E> HostUdpSocket for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn start_bind(
        &mut self,
        socket: Resource<UdpSocket>,
        _network: Resource<Network>,
        local_address: IpSocketAddress,
    ) -> Result<Result<(), ErrorCode>> {
        let addr = wit_socket_addr_to_std(&local_address);
        debug!("wasi:sockets/udp start-bind: {}", addr);

        self.record_handler_event(
            "wasi:sockets/udp/start-bind".to_string(),
            SocketsEventData::UdpStartBindCall { address: addr.to_string() },
            Some(format!("UDP start-bind to {}", addr)),
        );

        {
            let mut table = self.resource_table.lock().unwrap();
            let udp_socket: &mut UdpSocket = table.get_mut(&socket)?;

            match &udp_socket.state {
                UdpSocketState::Unbound => {
                    udp_socket.state = UdpSocketState::BindInProgress { local_address: addr };
                }
                _ => {
                    return Ok(Err(ErrorCode::InvalidState));
                }
            }
        }

        self.record_handler_event(
            "wasi:sockets/udp/start-bind".to_string(),
            SocketsEventData::UdpStartBindResult { success: true },
            Some("UDP start-bind initiated".to_string()),
        );

        Ok(Ok(()))
    }

    async fn finish_bind(&mut self, socket: Resource<UdpSocket>) -> Result<Result<(), ErrorCode>> {
        debug!("wasi:sockets/udp finish-bind");

        self.record_handler_event(
            "wasi:sockets/udp/finish-bind".to_string(),
            SocketsEventData::UdpFinishBindCall,
            Some("UDP finish-bind".to_string()),
        );

        let local_addr = {
            let table = self.resource_table.lock().unwrap();
            let udp_socket: &UdpSocket = table.get(&socket)?;
            match &udp_socket.state {
                UdpSocketState::BindInProgress { local_address } => *local_address,
                _ => return Ok(Err(ErrorCode::NotInProgress)),
            }
        };

        match TokioUdpSocket::bind(local_addr).await {
            Ok(tokio_socket) => {
                let actual_addr = tokio_socket.local_addr().unwrap_or(local_addr);
                let socket_arc = Arc::new(tokio_socket);

                {
                    let mut table = self.resource_table.lock().unwrap();
                    let udp_socket: &mut UdpSocket = table.get_mut(&socket)?;
                    udp_socket.state = UdpSocketState::Bound {
                        socket: socket_arc,
                        local_address: actual_addr,
                        remote_address: None,
                    };
                }

                self.record_handler_event(
                    "wasi:sockets/udp/finish-bind".to_string(),
                    SocketsEventData::UdpFinishBindResult { success: true },
                    Some(format!("UDP bound to {}", actual_addr)),
                );

                Ok(Ok(()))
            }
            Err(e) => {
                warn!("UDP bind failed: {}", e);

                self.record_handler_event(
                    "wasi:sockets/udp/finish-bind".to_string(),
                    SocketsEventData::UdpFinishBindResult { success: false },
                    Some(format!("UDP bind failed: {}", e)),
                );

                Ok(Err(ErrorCode::AddressInUse))
            }
        }
    }

    async fn stream(
        &mut self,
        socket: Resource<UdpSocket>,
        remote_address: Option<IpSocketAddress>,
    ) -> Result<Result<(Resource<IncomingDatagramStream>, Resource<OutgoingDatagramStream>), ErrorCode>> {
        let remote_addr = remote_address.map(|a| wit_socket_addr_to_std(&a));
        debug!("wasi:sockets/udp stream: remote={:?}", remote_addr);

        self.record_handler_event(
            "wasi:sockets/udp/stream".to_string(),
            SocketsEventData::UdpStreamCall { remote_address: remote_addr.map(|a| a.to_string()) },
            Some(format!("UDP stream: remote={:?}", remote_addr)),
        );

        let socket_arc = {
            let mut table = self.resource_table.lock().unwrap();
            let udp_socket: &mut UdpSocket = table.get_mut(&socket)?;

            match &mut udp_socket.state {
                UdpSocketState::Bound { socket, remote_address: ra, .. } => {
                    *ra = remote_addr;
                    Arc::clone(socket)
                }
                _ => return Ok(Err(ErrorCode::InvalidState)),
            }
        };

        let incoming = IncomingDatagramStream {
            socket: Arc::clone(&socket_arc),
            remote_address: remote_addr,
        };
        let outgoing = OutgoingDatagramStream {
            socket: socket_arc,
            remote_address: remote_addr,
            send_permit: u64::MAX,
        };

        let (incoming_res, outgoing_res) = {
            let mut table = self.resource_table.lock().unwrap();
            (table.push(incoming)?, table.push(outgoing)?)
        };

        self.record_handler_event(
            "wasi:sockets/udp/stream".to_string(),
            SocketsEventData::UdpStreamResult { success: true },
            Some("UDP stream created".to_string()),
        );

        Ok(Ok((incoming_res, outgoing_res)))
    }

    async fn local_address(&mut self, socket: Resource<UdpSocket>) -> Result<Result<IpSocketAddress, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let udp_socket: &UdpSocket = table.get(&socket)?;

        match udp_socket.local_address() {
            Some(addr) => Ok(Ok(std_socket_addr_to_wit(addr))),
            None => Ok(Err(ErrorCode::InvalidState)),
        }
    }

    async fn remote_address(&mut self, socket: Resource<UdpSocket>) -> Result<Result<IpSocketAddress, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let udp_socket: &UdpSocket = table.get(&socket)?;

        match udp_socket.remote_address() {
            Some(addr) => Ok(Ok(std_socket_addr_to_wit(addr))),
            None => Ok(Err(ErrorCode::InvalidState)),
        }
    }

    async fn address_family(&mut self, socket: Resource<UdpSocket>) -> Result<crate::bindings::wasi::sockets::network::IpAddressFamily> {
        let table = self.resource_table.lock().unwrap();
        let udp_socket: &UdpSocket = table.get(&socket)?;
        Ok(match udp_socket.family {
            IpAddressFamily::Ipv4 => crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv4,
            IpAddressFamily::Ipv6 => crate::bindings::wasi::sockets::network::IpAddressFamily::Ipv6,
        })
    }

    async fn unicast_hop_limit(&mut self, socket: Resource<UdpSocket>) -> Result<Result<u8, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let udp_socket: &UdpSocket = table.get(&socket)?;
        Ok(Ok(udp_socket.options.unicast_hop_limit))
    }

    async fn set_unicast_hop_limit(&mut self, socket: Resource<UdpSocket>, value: u8) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let udp_socket: &mut UdpSocket = table.get_mut(&socket)?;
        udp_socket.options.unicast_hop_limit = value;
        Ok(Ok(()))
    }

    async fn receive_buffer_size(&mut self, socket: Resource<UdpSocket>) -> Result<Result<u64, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let udp_socket: &UdpSocket = table.get(&socket)?;
        Ok(Ok(udp_socket.options.receive_buffer_size))
    }

    async fn set_receive_buffer_size(&mut self, socket: Resource<UdpSocket>, value: u64) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let udp_socket: &mut UdpSocket = table.get_mut(&socket)?;
        udp_socket.options.receive_buffer_size = value;
        Ok(Ok(()))
    }

    async fn send_buffer_size(&mut self, socket: Resource<UdpSocket>) -> Result<Result<u64, ErrorCode>> {
        let table = self.resource_table.lock().unwrap();
        let udp_socket: &UdpSocket = table.get(&socket)?;
        Ok(Ok(udp_socket.options.send_buffer_size))
    }

    async fn set_send_buffer_size(&mut self, socket: Resource<UdpSocket>, value: u64) -> Result<Result<(), ErrorCode>> {
        if value == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }
        let mut table = self.resource_table.lock().unwrap();
        let udp_socket: &mut UdpSocket = table.get_mut(&socket)?;
        udp_socket.options.send_buffer_size = value;
        Ok(Ok(()))
    }

    async fn subscribe(&mut self, socket: Resource<UdpSocket>) -> Result<Resource<SocketsPollable>> {
        let socket_id = socket.rep();
        let pollable = SocketsPollable::for_udp_socket(socket_id);
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn drop(&mut self, socket: Resource<UdpSocket>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(socket);
        Ok(())
    }
}

impl<E> HostIncomingDatagramStream for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn receive(
        &mut self,
        _stream: Resource<IncomingDatagramStream>,
        max_results: u64,
    ) -> Result<Result<Vec<IncomingDatagram>, ErrorCode>> {
        debug!("wasi:sockets/udp incoming-datagram-stream.receive: max={}", max_results);

        self.record_handler_event(
            "wasi:sockets/udp/receive".to_string(),
            SocketsEventData::UdpReceiveCall { max_results },
            Some(format!("UDP receive: max={}", max_results)),
        );

        if max_results == 0 {
            return Ok(Ok(Vec::new()));
        }

        // For now, return empty since async receive needs more work
        self.record_handler_event(
            "wasi:sockets/udp/receive".to_string(),
            SocketsEventData::UdpReceiveResult { datagrams_received: 0 },
            Some("UDP receive: no datagrams available".to_string()),
        );

        Ok(Ok(Vec::new()))
    }

    async fn subscribe(&mut self, stream: Resource<IncomingDatagramStream>) -> Result<Resource<SocketsPollable>> {
        let stream_id = stream.rep();
        let pollable = SocketsPollable::for_incoming_datagram(stream_id);
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn drop(&mut self, stream: Resource<IncomingDatagramStream>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(stream);
        Ok(())
    }
}

impl<E> HostOutgoingDatagramStream for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn check_send(&mut self, stream: Resource<OutgoingDatagramStream>) -> Result<Result<u64, ErrorCode>> {
        debug!("wasi:sockets/udp outgoing-datagram-stream.check-send");

        self.record_handler_event(
            "wasi:sockets/udp/check-send".to_string(),
            SocketsEventData::UdpCheckSendCall,
            Some("UDP check-send".to_string()),
        );

        let permit = {
            let table = self.resource_table.lock().unwrap();
            let outgoing: &OutgoingDatagramStream = table.get(&stream)?;
            outgoing.send_permit
        };

        self.record_handler_event(
            "wasi:sockets/udp/check-send".to_string(),
            SocketsEventData::UdpCheckSendResult { permit },
            Some(format!("UDP check-send: permit={}", permit)),
        );

        Ok(Ok(permit))
    }

    async fn send(
        &mut self,
        stream: Resource<OutgoingDatagramStream>,
        datagrams: Vec<OutgoingDatagram>,
    ) -> Result<Result<u64, ErrorCode>> {
        let num_datagrams = datagrams.len();
        debug!("wasi:sockets/udp outgoing-datagram-stream.send: {} datagrams", num_datagrams);

        self.record_handler_event(
            "wasi:sockets/udp/send".to_string(),
            SocketsEventData::UdpSendCall { num_datagrams },
            Some(format!("UDP send: {} datagrams", num_datagrams)),
        );

        if datagrams.is_empty() {
            return Ok(Ok(0));
        }

        let (socket_arc, default_remote) = {
            let table = self.resource_table.lock().unwrap();
            let outgoing: &OutgoingDatagramStream = table.get(&stream)?;
            (Arc::clone(&outgoing.socket), outgoing.remote_address)
        };

        let mut sent = 0u64;
        for datagram in datagrams {
            let remote = datagram.remote_address
                .map(|a| wit_socket_addr_to_std(&a))
                .or(default_remote);

            match remote {
                Some(addr) => {
                    match socket_arc.send_to(&datagram.data, addr).await {
                        Ok(_) => sent += 1,
                        Err(e) => {
                            warn!("UDP send error: {}", e);
                            break;
                        }
                    }
                }
                None => {
                    return Ok(Err(ErrorCode::InvalidArgument));
                }
            }
        }

        self.record_handler_event(
            "wasi:sockets/udp/send".to_string(),
            SocketsEventData::UdpSendResult { datagrams_sent: sent },
            Some(format!("UDP sent {} datagrams", sent)),
        );

        Ok(Ok(sent))
    }

    async fn subscribe(&mut self, stream: Resource<OutgoingDatagramStream>) -> Result<Resource<SocketsPollable>> {
        let stream_id = stream.rep();
        let pollable = SocketsPollable::for_outgoing_datagram(stream_id);
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn drop(&mut self, stream: Resource<OutgoingDatagramStream>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(stream);
        Ok(())
    }
}

// =============================================================================
// wasi:sockets/ip-name-lookup Host implementation
// =============================================================================

impl<E> IpNameLookupHost for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn resolve_addresses(
        &mut self,
        _network: Resource<Network>,
        name: String,
    ) -> Result<Result<Resource<ResolveAddressStream>, ErrorCode>> {
        debug!("wasi:sockets/ip-name-lookup resolve-addresses: {}", name);

        self.record_handler_event(
            "wasi:sockets/ip-name-lookup/resolve-addresses".to_string(),
            SocketsEventData::ResolveAddressesCall { name: name.clone() },
            Some(format!("DNS resolve: {}", name)),
        );

        // Create a resolve stream
        let mut resolve_stream = ResolveAddressStream::new();

        // Try to resolve the name (blocking for now, would be async in full impl)
        match (name.as_str(), 0u16).to_socket_addrs() {
            Ok(addrs) => {
                resolve_stream.addresses = addrs.map(|a| a.ip()).collect();
                resolve_stream.complete = true;
            }
            Err(e) => {
                warn!("DNS resolution failed for {}: {}", name, e);
                resolve_stream.error = Some(e.to_string());
                resolve_stream.complete = true;
            }
        }

        let success = resolve_stream.error.is_none();
        let resource = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(resolve_stream)?
        };

        self.record_handler_event(
            "wasi:sockets/ip-name-lookup/resolve-addresses".to_string(),
            SocketsEventData::ResolveAddressesResult { success },
            Some(format!("DNS resolve started: success={}", success)),
        );

        Ok(Ok(resource))
    }
}

impl<E> HostResolveAddressStream for ActorStore<E>
where
    E: EventPayload + Clone + From<SocketsEventData> + Send,
{
    async fn resolve_next_address(
        &mut self,
        stream: Resource<ResolveAddressStream>,
    ) -> Result<Result<Option<crate::bindings::wasi::sockets::network::IpAddress>, ErrorCode>> {
        debug!("wasi:sockets/ip-name-lookup resolve-next-address");

        self.record_handler_event(
            "wasi:sockets/ip-name-lookup/resolve-next-address".to_string(),
            SocketsEventData::ResolveNextAddressCall,
            Some("DNS resolve next".to_string()),
        );

        let result = {
            let mut table = self.resource_table.lock().unwrap();
            let resolve: &mut ResolveAddressStream = table.get_mut(&stream)?;

            if resolve.error.is_some() {
                return Ok(Err(ErrorCode::NameUnresolvable));
            }

            resolve.next_address().map(|ip| std_ip_to_wit(ip))
        };

        self.record_handler_event(
            "wasi:sockets/ip-name-lookup/resolve-next-address".to_string(),
            SocketsEventData::ResolveNextAddressResult { address: result.as_ref().map(|_| "...".to_string()) },
            Some(format!("DNS resolve next: has_address={}", result.is_some())),
        );

        Ok(Ok(result))
    }

    async fn subscribe(&mut self, stream: Resource<ResolveAddressStream>) -> Result<Resource<SocketsPollable>> {
        let stream_id = stream.rep();
        let pollable = SocketsPollable::for_resolve_address(stream_id);
        let mut table = self.resource_table.lock().unwrap();
        Ok(table.push(pollable)?)
    }

    async fn drop(&mut self, stream: Resource<ResolveAddressStream>) -> Result<()> {
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(stream);
        Ok(())
    }
}
