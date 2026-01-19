mod bindings;

use bindings::wasi::sockets::instance_network;
use bindings::wasi::sockets::ip_name_lookup;
use bindings::wasi::sockets::network::IpAddress;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(
        _state: Option<Vec<u8>>,
        _params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Test 1: Get the default network instance
        let network = instance_network::instance_network();

        // Test 2: Resolve a well-known hostname (localhost)
        match ip_name_lookup::resolve_addresses(&network, "localhost") {
            Ok(stream) => {
                // Try to get the first address
                // The stream is a resource that yields addresses
                let pollable = stream.subscribe();
                pollable.block(); // Wait for results

                match stream.resolve_next_address() {
                    Ok(Some(addr)) => {
                        match addr {
                            IpAddress::Ipv4((a, b, c, d)) => {
                                // localhost should resolve to 127.0.0.1
                                assert!(
                                    a == 127 && b == 0 && c == 0 && d == 1,
                                    "Expected localhost to resolve to 127.0.0.1, got {}.{}.{}.{}",
                                    a, b, c, d
                                );
                            }
                            IpAddress::Ipv6(_) => {
                                // IPv6 localhost is also valid (::1)
                            }
                        }
                    }
                    Ok(None) => {
                        return Err("resolve_next_address returned None".to_string());
                    }
                    Err(e) => {
                        return Err(format!("resolve_next_address failed: {:?}", e));
                    }
                }
            }
            Err(e) => {
                return Err(format!("Failed to resolve localhost: {:?}", e));
            }
        }

        Ok((Some(b"WASI sockets DNS tests passed!".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
