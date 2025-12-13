# theater-handler-http-client

HTTP client handler for Theater WebAssembly actors.

## Overview

This handler provides HTTP client capabilities for WebAssembly actors running in the Theater runtime. It allows actors to make HTTP requests to external services while maintaining security controls through permission-based access and logging all operations to the actor's chain for debugging and auditing.

## Features

- **Full HTTP method support**: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS, etc.
- **Headers and body**: Complete control over request headers and body content
- **Permission-based access**: Control which hosts and HTTP methods actors can use
- **Event logging**: All HTTP requests and responses are logged to the chain
- **Error handling**: Graceful handling of network errors and invalid requests

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
theater-handler-http-client = "0.2.1"
```

### Basic Example

```rust
use theater_handler_http_client::HttpClientHandler;
use theater::config::actor_manifest::HttpClientHandlerConfig;

// Create handler with default config
let config = HttpClientHandlerConfig {};
let handler = HttpClientHandler::new(config, None);
```

### With Permissions

```rust
use theater_handler_http_client::HttpClientHandler;
use theater::config::actor_manifest::HttpClientHandlerConfig;
use theater::config::permissions::HttpClientPermissions;

// Create permissions allowing specific hosts and methods
let permissions = HttpClientPermissions {
    allowed_hosts: vec!["api.example.com".to_string()],
    denied_hosts: vec!["malicious.com".to_string()],
    allowed_methods: vec!["GET".to_string(), "POST".to_string()],
};

let config = HttpClientHandlerConfig {};
let handler = HttpClientHandler::new(config, Some(permissions));
```

## WIT Interface

This handler implements the `theater:simple/http-client` interface:

```wit
interface http-client {
    record http-request {
        method: string,
        uri: string,
        headers: list<tuple<string, string>>,
        body: option<list<u8>>,
    }
    
    record http-response {
        status: u16,
        headers: list<tuple<string, string>>,
        body: option<list<u8>>,
    }
    
    // Send an HTTP request and receive a response
    send-http: func(request: http-request) -> result<http-response, string>;
}
```

## Configuration

### HttpClientHandlerConfig

Currently has no configuration options. Future versions may add:
- Timeout settings
- Connection pooling options
- TLS configuration

### HttpClientPermissions

- `allowed_hosts`: List of specific hosts that actors can access (exact match or wildcard)
- `denied_hosts`: List of hosts that actors cannot access (takes precedence)
- `allowed_methods`: List of HTTP methods that actors can use

If no permissions are provided, all hosts and methods are accessible.

## Chain Events

All HTTP operations are logged as chain events:

- `HandlerSetupStart`: Handler initialization begins
- `LinkerInstanceSuccess`: WASM linker setup successful
- `HttpClientRequestCall`: HTTP request initiated
- `HttpClientRequestResult`: HTTP response received
- `PermissionDenied`: Access denied due to permissions
- `Error`: Request error occurred
- `HandlerSetupSuccess`: Handler setup completed

## Security Considerations

1. **Permission checking**: All requests are checked against configured permissions before execution
2. **Host validation**: URLs are parsed and hosts are validated
3. **Logging**: All requests and responses are logged for auditing
4. **Error handling**: Network errors and invalid requests are handled gracefully
5. **No automatic redirects**: Actors must explicitly handle redirects if needed

## Example Actor Usage

From within a WebAssembly actor:

```rust
// Make a GET request
let request = HttpRequest {
    method: "GET".to_string(),
    uri: "https://api.example.com/data".to_string(),
    headers: vec![
        ("User-Agent".to_string(), "Theater-Actor/1.0".to_string()),
    ],
    body: None,
};

match send_http(request) {
    Ok(response) => {
        println!("Status: {}", response.status);
        if let Some(body) = response.body {
            println!("Body: {}", String::from_utf8_lossy(&body));
        }
    }
    Err(e) => {
        println!("Request failed: {}", e);
    }
}
```

## Migration Notes

This handler was migrated from the core `theater` crate as part of the handler modularization effort. The migration included:

- Renamed from `HttpClientHost` to `HttpClientHandler`
- Implemented the `Handler` trait
- Made `setup_host_functions` synchronous (wrapper around async operation)
- Added `Clone` derive for handler reusability
- Improved documentation

## Dependencies

- `reqwest` - HTTP client library
- `wasmtime` - WebAssembly runtime with component model support
- `theater` - Core theater types and traits

## License

Apache-2.0
