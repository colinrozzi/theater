# WASI HTTP Handler

The WASI HTTP Handler provides standard WASI HTTP interfaces for actors, enabling them to make outgoing HTTP requests using the official `wasi:http` specification.

## Overview

Unlike the simpler [HTTP Client Handler](http-client.md) which uses Theater's custom interface, this handler implements the standard WASI HTTP interfaces:

- `wasi:http/types@0.2.0` - Core HTTP types and resources
- `wasi:http/outgoing-handler@0.2.0` - Outgoing request handling

This allows actors built with standard WASI HTTP tooling (like `wit-bindgen` with WASI targets) to run in Theater without modification.

## When to Use

Use the **WASI HTTP Handler** when:
- Your actor is built with standard WASI tooling
- You want compatibility with other WASI runtimes
- You need the full WASI HTTP resource model (streaming bodies, etc.)

Use the **HTTP Client Handler** when:
- You want a simpler API
- You're building Theater-specific actors
- You don't need WASI compatibility

## Configuration

The WASI HTTP handler is automatically activated when a component imports `wasi:http/types@0.2.0` or `wasi:http/outgoing-handler@0.2.0`. No manifest configuration is required.

## Interface

### Core Types

```wit
// HTTP method variant
variant method {
    get, head, post, put, delete,
    connect, options, trace, patch,
    other(string),
}

// URL scheme variant
variant scheme {
    HTTP, HTTPS, other(string),
}

// HTTP status code
type status-code = u16;
```

### Resources

#### fields (Headers)

```wit
resource fields {
    constructor();
    from-list: static func(entries: list<tuple<string, list<u8>>>) -> result<fields, header-error>;
    get: func(name: string) -> list<list<u8>>;
    set: func(name: string, value: list<list<u8>>) -> result<_, header-error>;
    delete: func(name: string) -> result<_, header-error>;
    append: func(name: string, value: list<u8>) -> result<_, header-error>;
    entries: func() -> list<tuple<string, list<u8>>>;
    clone: func() -> fields;
}
```

#### outgoing-request

```wit
resource outgoing-request {
    constructor(headers: fields);
    method: func() -> method;
    set-method: func(method: method) -> result;
    scheme: func() -> option<scheme>;
    set-scheme: func(scheme: option<scheme>) -> result;
    authority: func() -> option<string>;
    set-authority: func(authority: option<string>) -> result;
    path-with-query: func() -> option<string>;
    set-path-with-query: func(path: option<string>) -> result;
    headers: func() -> headers;
    body: func() -> result<outgoing-body, _>;
}
```

#### incoming-response

```wit
resource incoming-response {
    status: func() -> status-code;
    headers: func() -> headers;
    consume: func() -> result<incoming-body, _>;
}
```

### Outgoing Handler

```wit
interface outgoing-handler {
    handle: func(
        request: outgoing-request,
        options: option<request-options>
    ) -> result<future-incoming-response, error-code>;
}
```

## Usage Example

### Rust Actor with wit-bindgen

```rust
use wasi::http::types::{Fields, OutgoingRequest, Scheme, Method};
use wasi::http::outgoing_handler::handle;

pub fn fetch_data(url: &str) -> Result<Vec<u8>, String> {
    // Parse URL
    let (authority, path) = parse_url(url)?;

    // Create headers
    let headers = Fields::new();

    // Create request
    let request = OutgoingRequest::new(headers);
    request.set_method(&Method::Get)
        .map_err(|_| "Failed to set method")?;
    request.set_scheme(Some(&Scheme::Https))
        .map_err(|_| "Failed to set scheme")?;
    request.set_authority(Some(&authority))
        .map_err(|_| "Failed to set authority")?;
    request.set_path_with_query(Some(&path))
        .map_err(|_| "Failed to set path")?;

    // Send request
    let future_response = handle(request, None)
        .map_err(|e| format!("Request failed: {:?}", e))?;

    // Get response (blocks until ready)
    let response = future_response.get()
        .ok_or("No response available")?
        .map_err(|_| "Response retrieval error")?
        .map_err(|e| format!("HTTP error: {:?}", e))?;

    // Check status
    let status = response.status();
    if status < 200 || status >= 300 {
        return Err(format!("HTTP {}", status));
    }

    // Read body
    let body = response.consume()
        .map_err(|_| "Failed to get body")?;
    let stream = body.stream()
        .map_err(|_| "Failed to get stream")?;

    let mut data = Vec::new();
    loop {
        match stream.read(64 * 1024) {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => data.extend(chunk),
            Err(_) => break,
        }
    }

    Ok(data)
}
```

## Chain Events

The handler records HTTP operations in the actor's chain:

| Event | Description |
|-------|-------------|
| `OutgoingRequestCall` | HTTP request initiated, includes method and URI |

## Error Handling

The `error-code` variant includes detailed error information:

- `DNS-timeout` - DNS resolution timed out
- `DNS-error` - DNS resolution failed
- `connection-refused` - Server refused connection
- `connection-timeout` - Connection attempt timed out
- `TLS-protocol-error` - TLS handshake failed
- `HTTP-request-denied` - Request was denied
- `HTTP-response-incomplete` - Response was truncated
- `internal-error(string)` - Internal error with details

## Implementation Notes

1. **Automatic activation**: Handler activates when component imports WASI HTTP interfaces
2. **Async execution**: HTTP requests execute asynchronously; use `future-incoming-response.get()` to retrieve results
3. **Body streaming**: Large bodies can be read incrementally via `input-stream`

## Related Topics

- [HTTP Client Handler](http-client.md) - Simpler Theater-specific HTTP client
- [HTTP Framework Handler](http-framework.md) - For serving HTTP requests
- [WIT Debugging Guide](../../development/wit-debugging.md) - Troubleshooting WIT interface issues
