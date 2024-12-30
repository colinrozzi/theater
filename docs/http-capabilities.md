# HTTP Capabilities

## Overview

Theater provides two types of HTTP capabilities:
1. HTTP Server (`HttpServerHost`)
2. HTTP Client (via `HttpCapability`)

## HTTP Server

### Configuration
```toml
[[handlers]]
type = "Http-server"
config = { port = 8080 }
```

### Implementation
```rust
pub struct HttpServerHost {
    port: u16,
}

pub struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

pub struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

### Request Processing
1. HTTP request arrives
2. Converted to HttpRequest structure
3. Wrapped in Actor Event
4. Processed by actor
5. Response converted to HTTP response

## HTTP Client Capability

### Configuration
```rust
impl HttpCapability {
    async fn setup_host_functions(&self, linker: &mut Linker<Store>) -> Result<()>
}
```

### Available Functions
```rust
// Send HTTP request
fn http_send(address: String, msg: Vec<u8>) -> Vec<u8>;

// Log messages
fn log(msg: String);

// Send actor messages
fn send(address: String, msg: Vec<u8>);
```

### Request Format
```rust
struct HttpRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

### Response Format
```rust
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

## Usage Examples

### HTTP Server Handler
```rust
async fn handle_request(mut req: Request<mpsc::Sender<ActorMessage>>) -> tide::Result {
    let body_bytes = req.body_bytes().await?.to_vec();
    
    let http_request = HttpRequest {
        method: req.method().to_string(),
        uri: req.url().path().to_string(),
        headers: req.header_names()
            .map(|name| (
                name.to_string(),
                req.header(name).unwrap().first().unwrap().to_string(),
            ))
            .collect(),
        body: Some(body_bytes),
    };

    // Create and send event...
}
```

### HTTP Client Usage
```rust
// From component code
let request = HttpRequest {
    method: "GET".to_string(),
    url: "https://api.example.com/data".to_string(),
    headers: vec![
        ("Content-Type".to_string(), "application/json".to_string())
    ],
    body: None,
};

let response_bytes = http_send("https://api.example.com", 
    serde_json::to_vec(&request).unwrap());
let response: HttpResponse = serde_json::from_slice(&response_bytes).unwrap();
```

## Chain Integration

### Request Recording
```rust
let evt = Event {
    type_: "http_request".to_string(),
    data: json!(http_request),
};

chain_tx.send(ChainRequest {
    request_type: ChainRequestType::AddEvent { event: evt },
    response_tx: tx,
}).await?;
```

### Response Recording
```rust
let evt = Event {
    type_: "actor-message".to_string(),
    data: json!({
        "address": address,
        "message": response_bytes,
    }),
};
```

## Error Handling

### HTTP Errors
```rust
fn error_response(status: u16, message: &str) -> Response {
    Response::builder(status)
        .body(Body::from_string(message.to_string()))
        .build()
}
```

### Chain Errors
```rust
if let Err(e) = chain_tx.send(ChainRequest {...}).await {
    error!("Failed to record message in chain: {}", e);
    return error_response(500, "Internal chain error");
}
```

## Security Considerations

1. **Server Binding**
   - Binds to localhost by default
   - Port configuration via manifest
   - No TLS in current implementation

2. **Request Validation**
   - All requests recorded in chain
   - Headers and body validated
   - Method validation

3. **Response Security**
   - Headers validated
   - Body size limits
   - Error handling

## Limitations

Current limitations:
1. No built-in routing
2. Basic CORS support only
3. No WebSocket support
4. No streaming support
5. Limited middleware options

## Future Improvements

1. **Enhanced Routing**
   - Path parameters
   - Query handling
   - Router middleware

2. **Security Features**
   - TLS support
   - Authentication
   - Rate limiting
   - CORS configuration

3. **Protocol Support**
   - WebSocket handlers
   - Server-sent events
   - gRPC integration
   - GraphQL support