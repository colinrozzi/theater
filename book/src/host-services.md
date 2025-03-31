# Host Services Reference

Theater provides several host services that actors can use. Each service has its own configuration and available functions.

## Message Server

The message server handles actor-to-actor communication and event handling.

### Configuration
```toml
[[handlers]]
type = "message-server"
config = { 
    port = 8080,
    # Optional configurations
    host = "127.0.0.1",
}
```

### Available Functions
```rust
// Import the message server interface
use bindings::ntwk::theater::message_server::{
    send,           // Send message to another actor
    handle,         // Handle incoming messages
};

// Send a message to another actor
send("http://localhost:8000", &json!({
    "type": "update",
    "data": { "value": 42 }
}));

// Handle incoming messages
impl MessageServerGuest for Component {
    fn handle(message: Message, state: Json) -> Json {
        // Process message and return new state
    }
}
```

## HTTP Server

The HTTP server allows actors to handle HTTP requests and serve content.

### Configuration
```toml
[[handlers]]
type = "http-server"
config = { 
    port = 8081,
    # Optional configurations
    host = "127.0.0.1",
    path_prefix = "/api",
}
```

### Available Functions
```rust
use bindings::exports::ntwk::theater::http_server::{
    Guest as HttpGuest,
    HttpRequest,
    HttpResponse,
};

impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        // Access request fields
        let method = req.method;        // GET, POST, etc.
        let path = req.path;            // Request path
        let headers = req.headers;      // Request headers
        let body = req.body;            // Optional request body

        // Create response
        let response = HttpResponse {
            status: 200,
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string())
            ],
            body: Some(b"{}".to_vec()),
        };
        
        (response, state)
    }
}
```

## Filesystem Service

The filesystem service provides access to files within configured directories.

### Configuration
```toml
[[handlers]]
type = "filesystem"
config = { 
    path = "/path/to/assets",            # Base directory
}
```

### Available Functions
```rust
use bindings::ntwk::theater::filesystem::{
    read_file,       // Read file contents
    write_file,      // Write file contents (if allowed)
    list_dir,        // List directory contents
    file_exists,     // Check if file exists
};

// Read file contents
let content = read_file("assets/index.html");

// Write file (if configured)
write_file("output/data.json", &json_data);

// List directory
let entries = list_dir("assets");

// Check file existence
if file_exists("assets/style.css") {
    // File exists
}
```

## Common Host Service Patterns

### Combining Services

Actors often use multiple services together:

```rust
struct Component;

// Implement message handling
impl MessageServerGuest for Component {
    fn handle(message: Message, state: Json) -> Json {
        // Handle actor messages
    }
}

// Implement HTTP handling
impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        // Handle HTTP requests
    }
}

// Use filesystem in both
fn serve_file(path: &str) -> HttpResponse {
    let content = read_file(path);
    HttpResponse {
        status: 200,
        headers: vec![
            ("Content-Type".to_string(), "text/html".to_string())
        ],
        body: Some(content),
    }
}
```

### State Management Across Services

Each service handler can modify actor state:

```rust
#[derive(Serialize, Deserialize)]
struct State {
    data: Value,
    last_updated: String,
}

impl MessageServerGuest for Component {
    fn handle(message: Message, state: Json) -> Json {
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        // Update state via message
        serde_json::to_vec(&current_state).unwrap()
    }
}

impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        // Update state via HTTP
        let new_state = serde_json::to_vec(&current_state).unwrap();
        (response, new_state)
    }
}
```
