mod bindings;

use bindings::wasi::sockets::instance_network;
use bindings::wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use bindings::wasi::sockets::tcp_create_socket;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(
        _state: Option<Vec<u8>>,
        _params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Get the default network instance
        let network = instance_network::instance_network();

        // Create a TCP socket for IPv4
        let socket = tcp_create_socket::create_tcp_socket(IpAddressFamily::Ipv4)
            .map_err(|e| format!("Failed to create TCP socket: {:?}", e))?;

        // Test getting address family
        let family = socket.address_family();
        assert!(
            family == IpAddressFamily::Ipv4,
            "Expected IPv4 address family"
        );

        // Test setting keep-alive
        socket
            .set_keep_alive_enabled(true)
            .map_err(|e| format!("Failed to enable keep-alive: {:?}", e))?;

        let keep_alive = socket
            .keep_alive_enabled()
            .map_err(|e| format!("Failed to get keep-alive status: {:?}", e))?;
        assert!(keep_alive, "Expected keep-alive to be enabled");

        // Bind to localhost on an ephemeral port (port 0)
        let bind_addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
            address: (127, 0, 0, 1),
            port: 0,
        });

        socket
            .start_bind(&network, bind_addr)
            .map_err(|e| format!("Failed to start bind: {:?}", e))?;

        // Wait for bind to complete
        let bind_pollable = socket.subscribe();
        bind_pollable.block();

        socket
            .finish_bind()
            .map_err(|e| format!("Failed to finish bind: {:?}", e))?;

        // Get the local address (should have an assigned port now)
        let local_addr = socket
            .local_address()
            .map_err(|e| format!("Failed to get local address: {:?}", e))?;

        match local_addr {
            IpSocketAddress::Ipv4(addr) => {
                let (a, b, c, d) = addr.address;
                assert!(
                    a == 127 && b == 0 && c == 0 && d == 1,
                    "Expected 127.0.0.1"
                );
                assert!(addr.port > 0, "Expected non-zero port after bind");
            }
            _ => return Err("Expected IPv4 socket address".to_string()),
        }

        // Start listening for connections
        socket
            .start_listen()
            .map_err(|e| format!("Failed to start listen: {:?}", e))?;

        let listen_pollable = socket.subscribe();
        listen_pollable.block();

        socket
            .finish_listen()
            .map_err(|e| format!("Failed to finish listen: {:?}", e))?;

        Ok((Some(b"WASI sockets TCP tests passed!".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
