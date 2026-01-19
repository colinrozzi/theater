# theater-handler-http

WASI HTTP handler for Theater WebAssembly actors, implementing the standard `wasi:http` interfaces.

## Overview

This handler provides WASI-compliant HTTP capabilities for WebAssembly actors. Unlike the simpler `theater-handler-http-client`, this handler implements the full `wasi:http/types@0.2.0` and `wasi:http/outgoing-handler@0.2.0` interfaces, allowing actors built with standard WASI HTTP tooling to run in Theater.

## Features

- **Standard WASI HTTP interfaces**: Full implementation of `wasi:http/types@0.2.0` and `wasi:http/outgoing-handler@0.2.0`
- **Resource-based API**: Proper WASI resource management for requests, responses, headers, and bodies
- **Streaming support**: Input/output streams for request and response bodies
- **Async HTTP execution**: Non-blocking HTTP requests via `future-incoming-response`

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
theater-handler-http = "0.2.1"
```

### Basic Setup

```rust
use theater_handler_http::WasiHttpHandler;

// Create handler
let handler = WasiHttpHandler::new();
```

## WIT Interfaces

This handler implements the standard WASI HTTP interfaces:

### wasi:http/types@0.2.0

Defines core types and resources:

```wit
interface types {
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

    resource incoming-response {
        status: func() -> status-code;
        headers: func() -> headers;
        consume: func() -> result<incoming-body, _>;
    }

    // ... additional resources
}
```

### wasi:http/outgoing-handler@0.2.0

```wit
interface outgoing-handler {
    use types.{outgoing-request, request-options, future-incoming-response, error-code};

    handle: func(
        request: outgoing-request,
        options: option<request-options>
    ) -> result<future-incoming-response, error-code>;
}
```

## Implementation Notes

### Type Mappings

WASI HTTP uses variant types that must be properly mapped:

| WIT Type | Rust Type |
|----------|-----------|
| `method` | `WasiMethod` enum with GET, POST, PUT, etc. |
| `scheme` | `WasiScheme` enum with HTTP, HTTPS, Other |
| `header-error` | `WasiHeaderError` enum |
| `error-code` | `WasiErrorCode` enum with DNS, connection errors, etc. |

### Resources in Correct Interface

All resources (`fields`, `outgoing-request`, `incoming-response`, etc.) must be defined in `wasi:http/types@0.2.0`, not in `outgoing-handler`. The `outgoing-handler` interface only contains the `handle` function.

### Async Response Handling

The `future-incoming-response.get` method uses `tokio::task::block_in_place()` to safely block within an async context when waiting for HTTP responses.

## Development Notes

### Common Issues When Implementing WASI Handlers

1. **Type signature mismatches**: WIT variant types like `method` and `scheme` require proper enum implementations with `#[derive(ComponentType, Lift, Lower)]` and `#[component(variant)]`.

2. **Wrong interface location**: Resources must be in `wasi:http/types`, not spread across other interfaces. The linker error "component imports instance X, but a matching implementation was not found" often indicates resources are in the wrong interface.

3. **Return type precision**: Functions like `fields.append` must return the exact WIT type (e.g., `result<_, header-error>` not `Result<(), u32>`).

4. **Nested return types**: `future-incoming-response.get` returns `option<result<result<incoming-response, error-code>, ()>>` - this triple nesting is intentional per the WASI spec.

5. **Async runtime conflicts**: Using `runtime.block_on()` inside tokio causes panics. Use `tokio::task::block_in_place(|| Handle::current().block_on(...))` instead.

See the [WIT Debugging Guide](../../crates/theater/book/src/development/wit-debugging.md) for detailed troubleshooting steps.

## Chain Events

HTTP operations are logged as chain events:

- `HandlerSetupStart`: Handler initialization begins
- `LinkerInstanceSuccess`: WASM linker setup successful
- `OutgoingRequestCall`: HTTP request initiated (includes method, URI)
- `HandlerSetupSuccess`: Handler setup completed

## Example Actor Usage

From within a WebAssembly actor using WASI HTTP:

```rust
use wasi::http::types::{Fields, OutgoingRequest, Scheme, Method};
use wasi::http::outgoing_handler::handle;

fn make_request() -> Result<String, String> {
    // Create headers
    let headers = Fields::new();

    // Create request
    let request = OutgoingRequest::new(headers);
    request.set_method(&Method::Get).unwrap();
    request.set_scheme(Some(&Scheme::Https)).unwrap();
    request.set_authority(Some("httpbin.org")).unwrap();
    request.set_path_with_query(Some("/get")).unwrap();

    // Send request
    let future_response = handle(request, None)
        .map_err(|e| format!("Request failed: {:?}", e))?;

    // Wait for response
    let response = future_response.get()
        .ok_or("No response")?
        .map_err(|_| "Response error")?
        .map_err(|e| format!("HTTP error: {:?}", e))?;

    let status = response.status();
    Ok(format!("Status: {}", status))
}
```

## Dependencies

- `reqwest` - HTTP client library
- `wasmtime` - WebAssembly runtime with component model support
- `theater` - Core theater types and traits
- `tokio` - Async runtime

## License

Apache-2.0
