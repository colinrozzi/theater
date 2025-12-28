# WASI HTTP Implementation Design

**Status:** Design Phase
**Date:** December 28, 2025
**Author:** Claude
**Goal:** Implement full WASI HTTP 0.2 support in Theater for streaming HTTP proxy capabilities

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Component Breakdown](#component-breakdown)
4. [Resource Lifecycle](#resource-lifecycle)
5. [Data Flow](#data-flow)
6. [Implementation Phases](#implementation-phases)
7. [Integration Points](#integration-points)
8. [Challenges & Solutions](#challenges--solutions)
9. [Testing Strategy](#testing-strategy)
10. [Future Enhancements](#future-enhancements)

---

## Overview

### Current State

**Theater's HTTP Implementation:**
- `theater:simple/http-client` - Fully buffered HTTP client (uses reqwest)
- `theater:simple/http-framework` - Fully buffered HTTP server (uses Axum)
- 100MB hard limit on request/response bodies
- All data loaded into memory (`Vec<u8>`)
- Not WASI-compliant

**Limitations:**
- Cannot stream large files
- High memory usage for concurrent requests
- No backpressure mechanism
- Incompatible with WASI ecosystem

### Target State

**WASI HTTP 0.2 Implementation:**
- `wasi:http/types` - Standard request/response/body resources
- `wasi:http/incoming-handler` - Server-side streaming HTTP
- `wasi:http/outgoing-handler` - Client-side streaming HTTP
- `wasi:http/proxy` - Complete proxy world

**Benefits:**
- Streaming request/response bodies (constant memory usage)
- Zero-copy operations via `splice()`
- Standards-compliant (works with any WASI runtime)
- Backpressure through pollables
- Can proxy unlimited-size payloads

---

## Architecture

### High-Level Component Stack

```
┌─────────────────────────────────────────────────────────┐
│  WASM Actor (Component)                                 │
│  ┌──────────────────────────────────────────────────┐  │
│  │  Implements: wasi:http/incoming-handler          │  │
│  │  Imports: wasi:http/outgoing-handler             │  │
│  │           wasi:io/streams                        │  │
│  │           wasi:io/poll                           │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                         ↕ (WIT interface)
┌─────────────────────────────────────────────────────────┐
│  Theater Runtime                                        │
│  ┌──────────────────────────────────────────────────┐  │
│  │  theater-handler-http (NEW)                      │  │
│  │  - Implements wasi:http/types resources          │  │
│  │  - Provides outgoing-handler (HTTP client)       │  │
│  │  - Exports incoming-handler (HTTP server)        │  │
│  └──────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────┐  │
│  │  theater-handler-io                              │  │
│  │  - Implements wasi:io/streams (input/output)     │  │
│  │  - Implements wasi:io/error                      │  │
│  │  - Implements wasi:io/poll (moved from timing)   │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                         ↕
┌─────────────────────────────────────────────────────────┐
│  Platform I/O                                           │
│  - Axum HTTP server (incoming requests)                 │
│  - reqwest HTTP client (outgoing requests)              │
│  - Tokio async runtime                                  │
└─────────────────────────────────────────────────────────┘
```

### WIT Interface Hierarchy

```
wasi:http@0.2.0
├── types
│   ├── method, scheme, status-code (primitives)
│   ├── error-code, header-error (error types)
│   ├── fields (headers/trailers resource)
│   ├── incoming-request (resource)
│   ├── outgoing-request (resource)
│   ├── request-options (resource)
│   ├── incoming-response (resource)
│   ├── outgoing-response (resource)
│   ├── incoming-body (resource → uses input-stream)
│   ├── outgoing-body (resource → uses output-stream)
│   ├── future-trailers (resource → pollable)
│   ├── future-incoming-response (resource → pollable)
│   └── response-outparam (resource)
├── incoming-handler
│   └── handle(incoming-request, response-outparam)
└── outgoing-handler
    └── handle(outgoing-request, options) → future-incoming-response

wasi:io@0.2.3
├── error
│   └── error (resource with to-debug-string)
├── streams
│   ├── stream-error (variant)
│   ├── input-stream (resource)
│   └── output-stream (resource)
└── poll
    ├── pollable (resource)
    └── poll(list<pollable>) → list<u32>
```

---

## Component Breakdown

### 1. **theater-handler-io** (Foundation Layer)

**Status:** ✅ Partially Complete

**Location:** `/crates/theater-handler-io/`

**Responsibilities:**
- Provide `wasi:io/error` resource
- Provide `wasi:io/streams` resources (input-stream, output-stream)
- Provide `wasi:io/poll` resources (moved from theater-handler-timing)
- Bridge between Theater's sync world and WASM's async model

**Resources to Implement:**

#### `error` Resource
```rust
pub struct IoError {
    message: String,
    kind: Option<String>,
}

// WIT methods:
// to-debug-string() → string
```

**Status:** ✅ Complete (basic implementation)

#### `input-stream` Resource
```rust
pub struct InputStream {
    buffer: Arc<Mutex<InputStreamState>>,
}

// WIT methods:
// read(len: u64) → result<list<u8>, stream-error>
// blocking-read(len: u64) → result<list<u8>, stream-error>
// skip(len: u64) → result<u64, stream-error>
// blocking-skip(len: u64) → result<u64, stream-error>
// subscribe() → pollable
```

**Status:** 🔨 Partial (read/skip done, needs blocking variants & subscribe)

#### `output-stream` Resource
```rust
pub struct OutputStream {
    buffer: Arc<Mutex<OutputStreamState>>,
}

// WIT methods:
// check-write() → result<u64, stream-error>
// write(contents: list<u8>) → result<_, stream-error>
// blocking-write-and-flush(contents: list<u8>) → result<_, stream-error>
// flush() → result<_, stream-error>
// blocking-flush() → result<_, stream-error>
// subscribe() → pollable
// write-zeroes(len: u64) → result<_, stream-error>
// blocking-write-zeroes-and-flush(len: u64) → result<_, stream-error>
// splice(src: input-stream, len: u64) → result<u64, stream-error>
// blocking-splice(src: input-stream, len: u64) → result<u64, stream-error>
```

**Status:** 🔨 Partial (basic write/flush done, needs blocking variants, splice, subscribe)

#### `pollable` Resource
```rust
pub struct Pollable {
    kind: PollableKind,
}

pub enum PollableKind {
    StreamReadable(Weak<Mutex<InputStreamState>>),
    StreamWritable(Weak<Mutex<OutputStreamState>>),
    Timer(Instant),
    HttpResponse(Weak<Mutex<ResponseState>>),
}

// WIT methods:
// ready() → bool
// block()
```

**Status:** ⏳ TODO (move from theater-handler-timing, extend for streams)

---

### 2. **theater-handler-http** (HTTP Layer)

**Status:** ⏳ Not Started

**Location:** `/crates/theater-handler-http/` (new crate)

**Responsibilities:**
- Provide all `wasi:http/types` resources
- Implement `wasi:http/outgoing-handler` (HTTP client with streaming)
- Handle export functions for `wasi:http/incoming-handler` (HTTP server with streaming)
- Bridge between Theater's Axum server and WASI streaming model
- Bridge between reqwest client and WASI streaming model

**Resources to Implement:**

#### `fields` Resource (Headers/Trailers)
```rust
pub struct Fields {
    entries: Vec<(String, Vec<u8>)>,
    immutable: bool,
}

// WIT methods:
// [constructor]() → fields
// from-list(entries: list<tuple<field-name, field-value>>) → result<fields, header-error>
// get(name: field-name) → list<field-value>
// has(name: field-name) → bool
// set(name: field-name, value: list<field-value>) → result<_, header-error>
// delete(name: field-name) → result<_, header-error>
// append(name: field-name, value: field-value) → result<_, header-error>
// entries() → list<tuple<field-name, field-value>>
// clone() → fields
```

**Complexity:** Medium (mutable/immutable distinction, case-insensitive lookup)

#### `incoming-request` Resource
```rust
pub struct IncomingRequest {
    method: Method,
    path_with_query: Option<String>,
    scheme: Option<Scheme>,
    authority: Option<String>,
    headers: Fields, // immutable
    body: Option<IncomingBody>,
}

// WIT methods:
// method() → method
// path-with-query() → option<string>
// scheme() → option<scheme>
// authority() → option<string>
// headers() → fields (immutable)
// consume() → result<incoming-body, _>
```

**Complexity:** Low (mostly getters)

#### `outgoing-request` Resource
```rust
pub struct OutgoingRequest {
    method: Method,
    path_with_query: Option<String>,
    scheme: Option<Scheme>,
    authority: Option<String>,
    headers: Fields, // mutable
}

// WIT methods:
// [constructor](headers: fields) → outgoing-request
// body() → result<outgoing-body, _>
// method() → method
// set-method(method: method) → result<_, _>
// path-with-query() → option<string>
// set-path-with-query(path: option<string>) → result<_, _>
// scheme() → option<scheme>
// set-scheme(scheme: option<scheme>) → result<_, _>
// authority() → option<string>
// set-authority(authority: option<string>) → result<_, _>
// headers() → fields
```

**Complexity:** Low-Medium (mutable state management)

#### `request-options` Resource
```rust
pub struct RequestOptions {
    connect_timeout: Option<Duration>,
    first_byte_timeout: Option<Duration>,
    between_bytes_timeout: Option<Duration>,
}

// WIT methods:
// [constructor]() → request-options
// connect-timeout() → option<duration>
// set-connect-timeout(duration: option<duration>) → result<_, _>
// first-byte-timeout() → option<duration>
// set-first-byte-timeout(duration: option<duration>) → result<_, _>
// between-bytes-timeout() → option<duration>
// set-between-bytes-timeout(duration: option<duration>) → result<_, _>
```

**Complexity:** Low (simple getters/setters)

#### `incoming-response` Resource
```rust
pub struct IncomingResponse {
    status: StatusCode,
    headers: Fields, // immutable
    body: Option<IncomingBody>,
}

// WIT methods:
// status() → status-code
// headers() → fields (immutable)
// consume() → result<incoming-body, _>
```

**Complexity:** Low

#### `outgoing-response` Resource
```rust
pub struct OutgoingResponse {
    status: StatusCode,
    headers: Fields, // mutable
}

// WIT methods:
// [constructor](headers: fields) → outgoing-response
// status-code() → status-code
// set-status-code(status: status-code) → result<_, _>
// headers() → fields
// body() → result<outgoing-body, _>
```

**Complexity:** Low-Medium

#### `incoming-body` Resource (CRITICAL - Integrates with Streams)
```rust
pub struct IncomingBody {
    stream: InputStream, // From wasi:io/streams!
    trailers: Option<FutureTrailers>,
}

// WIT methods:
// stream() → result<input-stream, _>
// finish(this: incoming-body) → future-trailers
```

**Complexity:** High (bridge between Axum/reqwest and WASI streams)

**Key Challenge:** Need to adapt platform I/O (Axum's Body, reqwest's Body) to WASI input-stream

**Design Options:**

**Option A: Buffered Adapter (Simpler)**
```rust
// When HTTP request arrives from Axum:
let axum_body = request.into_body();
let bytes = axum::body::to_bytes(axum_body).await?; // Buffer in memory
let stream = InputStream::from_bytes(bytes.to_vec());
let incoming_body = IncomingBody::new(stream);
```

**Pros:** Simple, works immediately
**Cons:** Not truly streaming, defeats the purpose

**Option B: Async Bridge (Proper Streaming)**
```rust
// When HTTP request arrives from Axum:
let axum_body = request.into_body();
let (tx, rx) = tokio::sync::mpsc::channel(16);

// Spawn task to read from Axum body and feed into channel
tokio::spawn(async move {
    while let Some(chunk) = axum_body.frame().await {
        tx.send(chunk).await.ok();
    }
});

// InputStream reads from channel on-demand
let stream = InputStream::from_channel(rx);
let incoming_body = IncomingBody::new(stream);
```

**Pros:** True streaming, constant memory
**Cons:** More complex, needs careful synchronization

**Recommendation:** Start with Option A for MVP, refactor to Option B for production

#### `outgoing-body` Resource (CRITICAL - Integrates with Streams)
```rust
pub struct OutgoingBody {
    stream: OutputStream, // From wasi:io/streams!
}

// WIT methods:
// write() → result<output-stream, _>
```

**Complexity:** High (bridge between WASI streams and Axum/reqwest)

**Key Challenge:** Need to adapt WASI output-stream to platform I/O

**Design:** Actor writes to OutputStream → we need to pipe that to Axum response or reqwest request

#### `future-trailers` Resource
```rust
pub struct FutureTrailers {
    state: Arc<Mutex<FutureState<Option<Fields>>>>,
}

// WIT methods:
// subscribe() → pollable
// get() → option<result<option<fields>, error-code>>
```

**Complexity:** Medium (async future pattern)

#### `future-incoming-response` Resource
```rust
pub struct FutureIncomingResponse {
    state: Arc<Mutex<FutureState<IncomingResponse>>>,
}

// WIT methods:
// subscribe() → pollable
// get() → option<result<incoming-response, error-code>>
```

**Complexity:** Medium-High (async HTTP client integration)

**Key Challenge:** Integrate with reqwest's async response model

#### `response-outparam` Resource
```rust
pub struct ResponseOutparam {
    response_tx: tokio::sync::oneshot::Sender<OutgoingResponse>,
}

// WIT methods:
// set(response: outgoing-response) → result<_, _>
```

**Complexity:** Medium (one-shot channel for response)

---

### 3. **Handler Implementations**

#### `outgoing-handler` Implementation (HTTP Client)

**Location:** `theater-handler-http/src/outgoing_handler.rs`

**WIT Interface:**
```wit
handle: func(
    request: outgoing-request,
    options: option<request-options>
) -> result<future-incoming-response, error-code>;
```

**Flow:**
```
1. Actor calls handle(outgoing-request, options)
2. Extract method, URL, headers from outgoing-request
3. Get outgoing-body.write() → output-stream
4. Collect data from output-stream (actor writes to it)
5. Build reqwest::Request with collected data
6. Spawn async task to send request via reqwest
7. Create future-incoming-response
8. When reqwest responds, populate future with incoming-response
9. incoming-response.body → adapt reqwest Body to InputStream
10. Return future-incoming-response immediately
```

**Pseudo-code:**
```rust
fn handle(
    request: OutgoingRequest,
    options: Option<RequestOptions>,
) -> Result<FutureIncomingResponse, ErrorCode> {
    let method = request.method();
    let url = build_url(&request)?;
    let headers = request.headers();

    // Get body stream
    let body_stream = request.body()?;
    let output_stream = body_stream.write()?;

    // Create future for response
    let (future_response, response_setter) = FutureIncomingResponse::new();

    // Spawn task to execute HTTP request
    tokio::spawn(async move {
        // Wait for actor to finish writing body
        let body_bytes = output_stream.collect().await;

        // Build reqwest request
        let req = reqwest::Client::new()
            .request(method, url)
            .headers(headers)
            .body(body_bytes)
            .build()?;

        // Execute request
        let resp = client.execute(req).await?;

        // Convert to incoming-response
        let status = resp.status().as_u16();
        let headers = Fields::from_reqwest_headers(resp.headers());

        // Adapt reqwest body to InputStream
        let body_stream = InputStream::from_reqwest_body(resp);
        let incoming_body = IncomingBody::new(body_stream);

        let incoming_response = IncomingResponse::new(status, headers, incoming_body);

        // Fulfill future
        response_setter.set(Ok(incoming_response));
    });

    Ok(future_response)
}
```

#### `incoming-handler` Export (HTTP Server)

**Location:** `theater-handler-http/src/incoming_handler.rs`

**WIT Interface:**
```wit
handle: func(
    request: incoming-request,
    response-out: response-outparam
);
```

**Flow:**
```
1. Axum receives HTTP request
2. Convert to incoming-request:
   - Extract method, path, scheme, authority
   - Create Fields from Axum headers
   - Adapt Axum Body to InputStream
   - Create IncomingBody with InputStream
3. Create response-outparam with oneshot channel
4. Call actor's exported handle(incoming-request, response-outparam)
5. Actor processes request, writes to response body stream
6. Actor calls response-outparam.set(outgoing-response)
7. Extract status, headers, body from outgoing-response
8. Convert body's OutputStream to Axum Body
9. Send Axum response
```

**Pseudo-code:**
```rust
async fn handle_axum_request(
    request: axum::http::Request<axum::body::Body>,
) -> axum::http::Response<axum::body::Body> {
    // Convert Axum request to incoming-request
    let method = Method::from_axum(request.method());
    let path = request.uri().path_and_query().map(|p| p.to_string());
    let scheme = Scheme::from_axum(request.uri().scheme());
    let authority = request.uri().authority().map(|a| a.to_string());
    let headers = Fields::from_axum_headers(request.headers());

    // Adapt Axum body to InputStream
    let body_stream = InputStream::from_axum_body(request.into_body());
    let incoming_body = IncomingBody::new(body_stream);

    let incoming_request = IncomingRequest::new(
        method, path, scheme, authority, headers, incoming_body
    );

    // Create response channel
    let (response_tx, response_rx) = oneshot::channel();
    let response_outparam = ResponseOutparam::new(response_tx);

    // Call actor's exported handle function
    actor_instance.handle(incoming_request, response_outparam)?;

    // Wait for actor to set response
    let outgoing_response = response_rx.await?;

    // Convert outgoing-response to Axum response
    let status = outgoing_response.status_code();
    let headers = outgoing_response.headers();
    let body_stream = outgoing_response.body()?.write()?;

    // Adapt OutputStream to Axum Body
    let body = Body::from_output_stream(body_stream).await?;

    axum::http::Response::builder()
        .status(status)
        .headers(headers)
        .body(body)
        .unwrap()
}
```

---

## Resource Lifecycle

### Resource Creation and Ownership

**Theater's ResourceTable** (per-actor):
```rust
pub struct ActorStore<E> {
    // Existing fields...
    pub resource_table: Arc<Mutex<ResourceTable>>,
}
```

**Resource Lifetime Rules:**
1. Resources are created by handler host functions
2. Stored in actor's ResourceTable
3. Referenced by WASM via `Resource<T>` handles (u32)
4. Dropped when actor drops handle or actor terminates
5. Parent-child relationships tracked (e.g., body owns stream)

**Example Resource Tree:**
```
incoming-request (parent)
├── headers (child - immutable Fields)
└── incoming-body (child)
    ├── input-stream (child)
    └── future-trailers (child)
        └── pollable (child)
```

### Drop Semantics

**Critical:** WASM components can drop resources at any time

**Handler must:**
1. Implement drop/destructor for each resource
2. Clean up platform resources (close streams, cancel HTTP requests)
3. Notify dependent resources
4. Remove from ResourceTable

**Example:**
```rust
// When input-stream is dropped:
fn drop_input_stream(rep: u32) {
    if let Some(stream) = resource_table.delete(rep) {
        stream.close(); // Signal EOF
        stream.notify_readers(); // Wake any blocked readers
    }
}
```

---

## Data Flow

### Example: Streaming HTTP Proxy Request Flow

```
┌──────────────────────────────────────────────────────────────────┐
│ 1. Client sends HTTP POST to proxy on port 8080                 │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 2. Axum server receives request                                  │
│    - Method: POST                                                │
│    - Path: /upload                                               │
│    - Body: 1GB file (streaming from client)                      │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 3. theater-handler-http converts to WASI incoming-request        │
│    - Creates Fields from Axum headers                            │
│    - Wraps Axum Body in InputStream (adapter)                    │
│    - Creates IncomingBody with InputStream                       │
│    - Creates ResponseOutparam (oneshot channel)                  │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 4. Call actor's incoming-handler.handle()                        │
│    handle(incoming-request, response-outparam)                   │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 5. Actor (WASM) processes request                                │
│    let body = incoming-request.consume()?;                       │
│    let stream = body.stream()?; // Get input-stream              │
│                                                                  │
│    // Create upstream request                                    │
│    let upstream_req = OutgoingRequest::new(headers);             │
│    upstream_req.set_method(Method::POST);                        │
│    upstream_req.set_path("/upload");                             │
│    let upstream_body = upstream_req.body()?;                     │
│    let upstream_stream = upstream_body.write()?;                 │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 6. Actor performs zero-copy streaming                            │
│    upstream_stream.splice(stream, 1GB)?;                         │
│    // This transfers data directly without buffering!            │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 7. outgoing-handler.handle() executes upstream request           │
│    let future_response = outgoing-handler.handle(                │
│        upstream_req, options                                     │
│    )?;                                                           │
│    // Spawns async task with reqwest                             │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 8. Actor waits for response                                      │
│    let pollable = future_response.subscribe();                   │
│    pollable.block(); // Wait until ready                         │
│    let response = future_response.get()?;                        │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 9. Actor forwards response back to client                        │
│    let resp_body = response.consume()?;                          │
│    let resp_stream = resp_body.stream()?;                        │
│                                                                  │
│    let outgoing = OutgoingResponse::new(headers);                │
│    outgoing.set_status_code(response.status());                  │
│    let out_body = outgoing.body()?;                              │
│    let out_stream = out_body.write()?;                           │
│                                                                  │
│    out_stream.splice(resp_stream, MAX)?; // Stream back!         │
│                                                                  │
│    response_outparam.set(outgoing)?;                             │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 10. theater-handler-http converts to Axum response               │
│     - Extracts status, headers                                   │
│     - Adapts OutputStream to Axum Body                           │
│     - Sends response                                             │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│ 11. Client receives streaming response                           │
└──────────────────────────────────────────────────────────────────┘
```

**Key Points:**
- No 100MB limit - constant memory usage
- Zero-copy splice operations
- True streaming - data flows as it arrives
- Backpressure via pollables

---

## Implementation Phases

### Phase 1: Foundation (Week 1)
**Goal:** Complete wasi:io handler

- [x] Create theater-handler-io crate structure
- [x] Implement IoError resource
- [x] Implement basic InputStream (read, skip)
- [x] Implement basic OutputStream (write, flush, check-write)
- [ ] Wire resources to wasmtime ResourceTable
- [ ] Implement blocking-read, blocking-write variants
- [ ] Implement splice operations
- [ ] Move pollable from timing handler to io handler
- [ ] Implement subscribe() methods for streams
- [ ] Create simple test actor using streams
- [ ] Validate stream operations

**Deliverable:** Working wasi:io/streams implementation

### Phase 2: HTTP Types Foundation (Week 2)
**Goal:** Implement core HTTP resource types

- [ ] Create theater-handler-http crate
- [ ] Copy wasi-http WIT files to theater/wit/
- [ ] Implement Fields resource (headers/trailers)
  - [ ] Constructor, from-list
  - [ ] get, has, set, delete, append
  - [ ] entries, clone
  - [ ] Immutability support
- [ ] Implement request-options resource
- [ ] Implement error-code enum and conversions
- [ ] Write unit tests for each resource

**Deliverable:** HTTP types foundation

### Phase 3: Request/Response Resources (Week 2-3)
**Goal:** Implement request and response types

- [ ] Implement incoming-request resource
  - [ ] method, path-with-query, scheme, authority getters
  - [ ] Immutable headers access
  - [ ] consume() method
- [ ] Implement outgoing-request resource
  - [ ] Constructor
  - [ ] All getters and setters
  - [ ] Mutable headers access
  - [ ] body() method
- [ ] Implement incoming-response resource
- [ ] Implement outgoing-response resource
- [ ] Implement response-outparam resource
- [ ] Write unit tests

**Deliverable:** Complete request/response types

### Phase 4: Body Resources (Week 3)
**Goal:** Integrate streams with HTTP bodies

**Critical Phase - This is where streaming happens!**

- [ ] Implement incoming-body resource
  - [ ] stream() returns wasi:io input-stream
  - [ ] finish() returns future-trailers
  - [ ] Integration with InputStream
- [ ] Implement outgoing-body resource
  - [ ] write() returns wasi:io output-stream
  - [ ] Integration with OutputStream
- [ ] Implement future-trailers resource
  - [ ] Async future pattern
  - [ ] subscribe() returns pollable
  - [ ] get() retrieves result
- [ ] Design stream adapters:
  - [ ] Axum Body → InputStream adapter
  - [ ] OutputStream → Axum Body adapter
  - [ ] reqwest Body → InputStream adapter
  - [ ] OutputStream → reqwest Body adapter
- [ ] Implement adapters
- [ ] Test with small payloads
- [ ] Test with large payloads (>100MB)
- [ ] Validate no buffering occurs

**Deliverable:** Streaming HTTP bodies

### Phase 5: Outgoing Handler (Week 4)
**Goal:** HTTP client with streaming

- [ ] Implement future-incoming-response resource
  - [ ] Async future pattern
  - [ ] Integration with tokio/reqwest
  - [ ] subscribe() returns pollable
  - [ ] get() retrieves response
- [ ] Implement outgoing-handler.handle()
  - [ ] Extract request details
  - [ ] Spawn async reqwest task
  - [ ] Return future immediately
  - [ ] Populate future when response arrives
- [ ] Handle timeouts (request-options)
- [ ] Handle errors (DNS, connection, HTTP)
- [ ] Test simple GET request
- [ ] Test POST with body
- [ ] Test streaming download (large file)
- [ ] Test error cases

**Deliverable:** Working HTTP client with streaming

### Phase 6: Incoming Handler (Week 4-5)
**Goal:** HTTP server with streaming

- [ ] Integrate with theater-handler-http-framework
  - [ ] Or create new Axum integration
  - [ ] Decision: Retrofit existing or new handler?
- [ ] Implement request conversion (Axum → WASI)
- [ ] Implement response conversion (WASI → Axum)
- [ ] Call actor's exported handle function
- [ ] Handle actor errors gracefully
- [ ] Test simple GET request
- [ ] Test POST with streaming upload
- [ ] Test streaming download response
- [ ] Test concurrent requests

**Deliverable:** Working HTTP server with streaming

### Phase 7: Integration & Testing (Week 5)
**Goal:** End-to-end validation

- [ ] Create WASI HTTP proxy test actor
  - [ ] Implement incoming-handler (server)
  - [ ] Import outgoing-handler (client)
  - [ ] Forward requests with streaming
  - [ ] Use splice for zero-copy
- [ ] Build test actor
- [ ] Deploy with Theater
- [ ] Test proxy scenarios:
  - [ ] Simple GET passthrough
  - [ ] POST with small body
  - [ ] POST with large body (>1GB)
  - [ ] Streaming download
  - [ ] Multiple concurrent requests
  - [ ] Error handling
- [ ] Performance benchmarks:
  - [ ] Memory usage (should be constant)
  - [ ] Throughput
  - [ ] Latency
- [ ] Compare to buffered implementation

**Deliverable:** Production-ready WASI HTTP

### Phase 8: Documentation & Polish (Week 6)
**Goal:** Complete implementation

- [ ] Write comprehensive docs
- [ ] Create examples
- [ ] Update Theater README
- [ ] Write migration guide (theater:simple → wasi:http)
- [ ] Add configuration options
- [ ] Tune performance
- [ ] Address any bugs found in testing

**Deliverable:** Released feature

---

## Integration Points

### 1. **Wasmtime Component Model**

**ResourceTable Integration:**
```rust
// In setup_host_functions:
let mut interface = linker.instance("wasi:io/streams@0.2.3")?;

// Define input-stream resource
interface.resource(
    "input-stream",
    ResourceType::host::<InputStream>(),
    |_ctx, rep| Ok(()) // Destructor
)?;

// Define methods on input-stream
interface.func_wrap(
    "[method]input-stream.read",
    |mut ctx: StoreContextMut<ActorStore<E>>, (stream, len): (Resource<InputStream>, u64)| {
        let table = ctx.data_mut().resource_table.lock().unwrap();
        let input_stream = table.get(&stream)?;
        let data = input_stream.read(len)?;
        Ok((data,))
    }
)?;
```

### 2. **Axum HTTP Server Integration**

**Option A: Extend theater-handler-http-framework**
- Add WASI compatibility layer
- Support both theater:simple and wasi:http
- Migration path for existing actors

**Option B: New handler (Recommended)**
- Clean separation
- Focus on WASI compliance
- Simpler implementation

**Integration Point:**
```rust
// In theater-handler-http/src/server.rs
async fn axum_handler(
    request: axum::http::Request<Body>,
    actor_handle: Arc<ActorHandle>,
) -> axum::http::Response<Body> {
    // Convert Axum request → WASI incoming-request
    let wasi_request = convert_request(request).await?;

    // Create response channel
    let (tx, rx) = oneshot::channel();
    let response_outparam = ResponseOutparam::new(tx);

    // Call actor's handle function
    actor_handle.call_incoming_handler(wasi_request, response_outparam)?;

    // Wait for response
    let wasi_response = rx.await?;

    // Convert WASI outgoing-response → Axum response
    convert_response(wasi_response).await
}
```

### 3. **reqwest HTTP Client Integration**

**Streaming Request Body:**
```rust
use reqwest::Body;
use tokio_util::io::ReaderStream;

// Convert OutputStream to reqwest Body
async fn output_stream_to_body(stream: OutputStream) -> Body {
    // Create async reader from OutputStream
    let reader = OutputStreamReader::new(stream);

    // Wrap in ReaderStream
    let stream = ReaderStream::new(reader);

    // Convert to reqwest Body
    Body::wrap_stream(stream)
}
```

**Streaming Response Body:**
```rust
// Convert reqwest Response to InputStream
async fn response_to_input_stream(response: reqwest::Response) -> InputStream {
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let body = response.bytes_stream();

    // Spawn task to feed chunks into channel
    tokio::spawn(async move {
        let mut stream = body;
        while let Some(chunk) = stream.next().await {
            if let Ok(bytes) = chunk {
                tx.send(bytes).await.ok();
            }
        }
    });

    InputStream::from_channel(rx)
}
```

### 4. **Event Chain Integration**

**Event Logging for HTTP:**
- Log request start (method, URL, headers)
- Log request body chunks (size, not content)
- Log response received (status, headers)
- Log response body chunks
- Log errors

**Example:**
```rust
ctx.data_mut().record_handler_event(
    "wasi:http/outgoing-handler/handle".to_string(),
    HttpEventData::OutgoingRequestStart {
        method: method.to_string(),
        url: url.to_string(),
        headers_count: headers.len(),
    },
    Some(format!("Starting {} request to {}", method, url)),
);

// After response:
ctx.data_mut().record_handler_event(
    "wasi:http/outgoing-handler/response".to_string(),
    HttpEventData::IncomingResponseReceived {
        status_code: response.status(),
        headers_count: response.headers().len(),
        body_size: body.size_hint(),
    },
    Some(format!("Received response with status {}", response.status())),
);
```

---

## Challenges & Solutions

### Challenge 1: Blocking Operations in Async Context

**Problem:** WASI specifies `blocking-read`, `blocking-write`, etc. but Theater uses async Tokio runtime.

**Solution:** Use `tokio::task::spawn_blocking` for blocking operations
```rust
interface.func_wrap(
    "[method]input-stream.blocking-read",
    |mut ctx, (stream, len): (Resource<InputStream>, u64)| {
        let stream = get_stream(&ctx, &stream)?;

        // Spawn blocking task
        tokio::task::spawn_blocking(move || {
            stream.blocking_read(len)
        }).await?
    }
)?;
```

### Challenge 2: Stream Lifetime Management

**Problem:** WASM can drop stream resources while I/O is in progress

**Solution:** Use `Weak` references in background tasks
```rust
let stream_weak = Arc::downgrade(&stream);

tokio::spawn(async move {
    while let Some(stream) = stream_weak.upgrade() {
        // Do work with stream
        if stream.is_closed() {
            break;
        }
    }
    // Stream was dropped, clean up
});
```

### Challenge 3: Backpressure

**Problem:** Fast producer, slow consumer (or vice versa)

**Solution:** Use bounded channels and pollables
```rust
// Output stream with backpressure
let (tx, rx) = tokio::sync::mpsc::channel(16); // Bounded!

// When channel is full, check_write() returns 0
// Actor must subscribe() and wait for pollable to be ready
```

### Challenge 4: Zero-Copy Splice

**Problem:** `splice(input-stream, output-stream, len)` should not buffer

**Solution:** Direct pipe between streams
```rust
async fn splice(
    input: &InputStream,
    output: &OutputStream,
    len: u64,
) -> Result<u64, StreamError> {
    let mut transferred = 0;

    while transferred < len {
        let remaining = len - transferred;
        let chunk_size = remaining.min(8192); // 8KB chunks

        // Read from input
        let chunk = input.read(chunk_size).await?;
        if chunk.is_empty() {
            break; // EOF
        }

        // Write to output
        output.write(&chunk).await?;
        transferred += chunk.len() as u64;
    }

    Ok(transferred)
}
```

**Optimization:** Use `tokio::io::copy` for zero-copy when possible

### Challenge 5: Error Mapping

**Problem:** Many error types (IO, HTTP, WASI)

**Solution:** Comprehensive error-code enum
```rust
pub enum ErrorCode {
    DnsTimeout,
    DnsError,
    DestinationNotFound,
    DestinationUnavailable,
    DestinationIpProhibited,
    // ... many more
    HttpRequestDenied,
    HttpRequestBodySize,
    // ...
}

impl From<std::io::Error> for ErrorCode { /* ... */ }
impl From<reqwest::Error> for ErrorCode { /* ... */ }
impl From<StreamError> for ErrorCode { /* ... */ }
```

---

## Testing Strategy

### Unit Tests

**Per Resource:**
- Constructor tests
- Method tests (each WIT method)
- Error cases
- Edge cases (empty, large, concurrent)

**Example:**
```rust
#[test]
fn test_input_stream_read() {
    let data = vec![1, 2, 3, 4, 5];
    let stream = InputStream::from_bytes(data);

    let chunk1 = stream.read(2).unwrap();
    assert_eq!(chunk1, vec![1, 2]);

    let chunk2 = stream.read(2).unwrap();
    assert_eq!(chunk2, vec![3, 4]);

    let chunk3 = stream.read(10).unwrap();
    assert_eq!(chunk3, vec![5]); // Only 1 byte left

    let chunk4 = stream.read(1).unwrap();
    assert_eq!(chunk4, vec![]); // EOF

    assert_eq!(stream.read(1), Err(StreamError::Closed));
}
```

### Integration Tests

**Stream Integration:**
- InputStream → OutputStream (copy)
- Splice operations
- Pollable integration

**HTTP Integration:**
- Simple GET request
- POST with body
- Streaming upload
- Streaming download
- Error handling

### End-to-End Tests

**WASI HTTP Proxy:**
- Deploy proxy actor
- Send test requests
- Verify responses
- Measure memory usage
- Measure throughput

**Test Matrix:**
```
┌─────────────┬────────┬────────┬────────┬─────────┐
│ Scenario    │ Body   │ Size   │ Method │ Expected│
├─────────────┼────────┼────────┼────────┼─────────┤
│ Simple GET  │ None   │ -      │ GET    │ 200 OK  │
│ GET JSON    │ None   │ -      │ GET    │ JSON    │
│ POST small  │ JSON   │ 1KB    │ POST   │ Echo    │
│ POST medium │ Binary │ 10MB   │ POST   │ Echo    │
│ POST large  │ Binary │ 1GB    │ POST   │ Echo    │
│ Stream down │ File   │ 1GB    │ GET    │ File    │
│ Concurrent  │ Mixed  │ Mixed  │ Mixed  │ All OK  │
│ Error 404   │ None   │ -      │ GET    │ 404     │
│ Error 500   │ None   │ -      │ GET    │ 500     │
│ Timeout     │ None   │ -      │ GET    │ Timeout │
└─────────────┴────────┴────────┴────────┴─────────┘
```

### Performance Tests

**Benchmarks:**
- Throughput (requests/sec)
- Latency (p50, p95, p99)
- Memory usage (should be constant)
- CPU usage

**Comparison:**
- WASI HTTP vs theater:simple/http (buffered)
- Show memory savings with large payloads

---

## Future Enhancements

### Phase 9+: Advanced Features

**TLS/SSL:**
- Client cert authentication
- Custom CA certificates
- SNI support

**HTTP/2 and HTTP/3:**
- Multiplexing
- Server push
- QUIC support

**WebSocket Streaming:**
- Integrate wasi:sockets
- Streaming WebSocket frames

**Compression:**
- gzip, br, deflate
- Transparent compression/decompression

**Caching:**
- HTTP cache headers
- Conditional requests (ETags, If-Modified-Since)

**Connection Pooling:**
- Reuse connections
- Keep-alive

**Proxy Features:**
- Load balancing
- Retries
- Circuit breakers
- Rate limiting

---

## Success Criteria

### Minimum Viable Product (MVP)

✅ **wasi:io/streams** fully implemented
✅ **wasi:http/types** all resources working
✅ **outgoing-handler** can make streaming HTTP requests
✅ **incoming-handler** can receive streaming HTTP requests
✅ Test actor: simple HTTP proxy that forwards requests
✅ Streaming works: can proxy 1GB+ files with constant memory
✅ Event chain logs all HTTP operations

### Production Ready

✅ All error cases handled gracefully
✅ Concurrent requests supported
✅ Resource cleanup verified (no leaks)
✅ Performance benchmarks show improvement
✅ Documentation complete
✅ Examples provided
✅ Integration tests passing

### Future Goals

✅ HTTP/2 support
✅ WebSocket streaming
✅ TLS configuration
✅ Advanced proxy features

---

## Timeline Estimate

**Total:** 6 weeks for production-ready implementation

- Week 1: wasi:io foundation
- Week 2: HTTP types foundation + request/response resources
- Week 3: Body resources (critical streaming integration)
- Week 4: Outgoing handler + Incoming handler
- Week 5: Integration testing + performance tuning
- Week 6: Documentation + polish

**Note:** Timeline assumes focused full-time work. Adjust based on available time and priorities.

---

## Next Steps

1. **Review this design** - Get feedback, adjust as needed
2. **Start Phase 1** - Complete wasi:io handler
   - Wire InputStream/OutputStream to wasmtime resources
   - Implement blocking variants
   - Implement splice
   - Move pollable from timing handler
   - Test with simple stream actor
3. **Create Phase 2 task breakdown** - Detail HTTP types implementation
4. **Set up project tracking** - GitHub issues, milestones

---

## References

- [WASI HTTP Specification](https://github.com/WebAssembly/wasi-http)
- [WASI I/O Specification](https://github.com/WebAssembly/wasi-io)
- [Component Model Documentation](https://component-model.bytecodealliance.org/)
- [Wasmtime Book](https://docs.wasmtime.dev/)
- [Axum Documentation](https://docs.rs/axum/latest/axum/)
- [reqwest Documentation](https://docs.rs/reqwest/latest/reqwest/)
- [Theater Architecture](../README.md)
