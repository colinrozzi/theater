//! Generated bindings from WASI HTTP WIT interfaces
//!
//! This module uses wasmtime::component::bindgen! to generate type-safe Host traits
//! from the WASI HTTP 0.2.0 WIT definitions. This ensures our implementation matches
//! the WIT interface at compile time.

use wasmtime::component::bindgen;

bindgen!({
    world: "http-handler-host",
    path: "wit",
    with: {
        // Map wasi:io types to theater-handler-io implementations
        "wasi:io/streams/input-stream": theater_handler_io::InputStream,
        "wasi:io/streams/output-stream": theater_handler_io::OutputStream,
        "wasi:io/error/error": theater_handler_io::IoError,

        // Map wasi:http resource types to our backing types
        "wasi:http/types/fields": crate::types::HostFields,
        "wasi:http/types/outgoing-request": crate::types::HostOutgoingRequest,
        "wasi:http/types/outgoing-response": crate::types::HostOutgoingResponse,
        "wasi:http/types/incoming-request": crate::types::HostIncomingRequest,
        "wasi:http/types/incoming-response": crate::types::HostIncomingResponse,
        "wasi:http/types/outgoing-body": crate::types::HostOutgoingBody,
        "wasi:http/types/incoming-body": crate::types::HostIncomingBody,
        "wasi:http/types/response-outparam": crate::types::HostResponseOutparam,
        "wasi:http/types/future-incoming-response": crate::types::HostFutureIncomingResponse,
        "wasi:http/types/future-trailers": crate::types::HostFutureTrailers,
        "wasi:http/types/request-options": crate::types::HostRequestOptions,
    },
    // Enable async for all functions
    async: true,
    // Allow trapping on errors
    trappable_imports: true,
});

// The bindgen! macro generates:
// - wasi::http::types - HTTP types Host trait (we implement)
// - wasi::http::outgoing_handler - Outgoing handler Host trait (we implement)
// - wasi::io::* - IO types Host traits (we implement)
// - wasi::clocks::* - Clock types Host traits (we implement)
// - exports::wasi::http::incoming_handler - Guest export we call

// Re-export the generated types for easier access
pub use wasi::http::types::Host as HttpTypesHost;
pub use wasi::http::types::HostFields;
pub use wasi::http::types::HostOutgoingRequest;
pub use wasi::http::types::HostOutgoingResponse;
pub use wasi::http::types::HostOutgoingBody;
pub use wasi::http::types::HostIncomingRequest;
pub use wasi::http::types::HostIncomingResponse;
pub use wasi::http::types::HostIncomingBody;
pub use wasi::http::types::HostResponseOutparam;
pub use wasi::http::types::HostFutureTrailers;
pub use wasi::http::types::HostFutureIncomingResponse;
pub use wasi::http::types::HostRequestOptions;

pub use wasi::http::outgoing_handler::Host as OutgoingHandlerHost;

// Re-export IO Host traits for setting up IO interfaces
pub use wasi::io::error::Host as IoErrorHost;
pub use wasi::io::streams::Host as IoStreamsHost;
pub use wasi::io::streams::HostInputStream;
pub use wasi::io::streams::HostOutputStream;

// Re-export key types
pub use wasi::http::types::{
    Method, Scheme, HeaderError, ErrorCode, StatusCode,
    Fields, OutgoingRequest, OutgoingResponse, OutgoingBody,
    IncomingRequest, IncomingResponse, IncomingBody,
    ResponseOutparam, FutureTrailers, FutureIncomingResponse,
    RequestOptions,
};
