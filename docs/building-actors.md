# Building Actors Guide

This guide walks you through building actors in Theater, from basic concepts to advanced patterns.

## What is a Theater Actor?

A Theater actor is a WebAssembly component that:
- Maintains state as JSON
- Handles messages and events
- Can serve HTTP requests
- Creates verifiable state transitions

## Creating Your First Actor

### Project Structure
```
my-actor/
├── Cargo.toml
├── actor.toml        # Actor manifest
├── assets/          # Static assets
├── src/
│   ├── lib.rs       # Actor implementation
│   └── bindings.rs  # WIT bindings
└── wit/            # Interface definitions
```

### Basic Actor Implementation

```rust
use serde::{Deserialize, Serialize};
use bindings::exports::ntwk::theater::actor::Guest as ActorGuest;
use bindings::ntwk::theater::types::{Event, Json};
use bindings::ntwk::theater::runtime::log;

// Define your actor's state
#[derive(Serialize, Deserialize)]
struct State {
    count: i32,
    last_updated: String,
}

struct Component;

impl ActorGuest for Component {
    // Initialize actor state
    fn init() -> Vec<u8> {
        log("Initializing actor");
        
        let initial_state = State {
            count: 0,
            last_updated: chrono::Utc::now().to_string(),
        };
        
        serde_json::to_vec(&initial_state).unwrap()
    }

    // Handle incoming messages
    fn handle(evt: Event, state: Json) -> Json {
        log(&format!("Handling event: {:?}", evt));
        
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        
        // Handle the event and update state
        match serde_json::from_value(evt.data) {
            Ok(message) => {
                // Process message...
                current_state.last_updated = chrono::Utc::now().to_string();
            }
            Err(e) => log(&format!("Error parsing message: {}", e)),
        }
        
        serde_json::to_vec(&current_state).unwrap()
    }
}

// Export the component
bindings::export!(Component with_types_in bindings);
```

### Actor Manifest (actor.toml)

```toml
name = "my-actor"
version = "0.1.0"
description = "Example Theater actor"

component_path = "target/wasm32-unknown-unknown/release/my_actor.wasm"

[interface]
implements = "ntwk:actor/actor"
requires = []

[[handlers]]
type = "http-server"
config = { port = 8080 }

[[handlers]]
type = "filesystem"
config = { path = "assets" }
```

## Available Host Functions

Theater provides several host functions through WIT bindings:

### Runtime Functions
```rust
use bindings::ntwk::theater::runtime::{
    log,       // Log messages
    send,      // Send messages to other actors
    spawn,     // Spawn new actors
    get_chain, // Get hash chain entries
};
```

### HTTP Functions
```rust
use bindings::exports::ntwk::theater::http_server::{
    Guest as HttpGuest,
    HttpRequest,
    HttpResponse,
};

impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        // Handle HTTP request
        let response = HttpResponse {
            status: 200,
            headers: vec![
                ("Content-Type".to_string(), "text/html".to_string())
            ],
            body: Some(b"Hello, World!".to_vec()),
        };
        (response, state)
    }
}
```

### Filesystem Access
```rust
use bindings::ntwk::theater::filesystem::read_file;

// Read static files
let content = read_file("assets/index.html");
```

## State Management

Theater actors maintain their state as JSON:

```rust
#[derive(Serialize, Deserialize)]
struct State {
    // Define your state structure
    data: Value,
    metadata: HashMap<String, String>,
}

impl ActorGuest for Component {
    fn handle(evt: Event, state: Json) -> Json {
        // Parse current state
        let mut current_state: State = 
            serde_json::from_slice(&state).unwrap();
        
        // Update state based on event
        current_state.data = process_event(evt);
        
        // Return new state
        serde_json::to_vec(&current_state).unwrap()
    }
}
```

## Handling HTTP Requests

Theater actors can serve HTTP requests:

```rust
impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        match (req.method.as_str(), req.path.as_str()) {
            // Serve static content
            ("GET", "/") => {
                let index = read_file("assets/index.html");
                (
                    HttpResponse {
                        status: 200,
                        headers: vec![
                            ("Content-Type".to_string(), "text/html".to_string())
                        ],
                        body: Some(index),
                    },
                    state
                )
            },
            
            // Handle API requests
            ("POST", "/api/data") => {
                let mut current_state: State = 
                    serde_json::from_slice(&state).unwrap();
                
                // Update state based on request
                if let Some(body) = req.body {
                    current_state.data = 
                        serde_json::from_slice(&body).unwrap();
                }
                
                let new_state = serde_json::to_vec(&current_state).unwrap();
                
                (
                    HttpResponse {
                        status: 200,
                        headers: vec![
                            ("Content-Type".to_string(), 
                             "application/json".to_string())
                        ],
                        body: Some(b"{}".to_vec()),
                    },
                    new_state
                )
            },
            
            // Handle unknown routes
            _ => (
                HttpResponse {
                    status: 404,
                    headers: vec![],
                    body: None,
                },
                state
            ),
        }
    }
}
```

## Common Patterns

### Event Handling
```rust
#[derive(Deserialize)]
enum Message {
    Increment { amount: i32 },
    Reset,
    Update { data: Value },
}

fn handle_message(msg: Message, state: &mut State) -> Result<(), Error> {
    match msg {
        Message::Increment { amount } => {
            state.count += amount;
            Ok(())
        },
        Message::Reset => {
            state.count = 0;
            Ok(())
        },
        Message::Update { data } => {
            state.data = data;
            Ok(())
        },
    }
}
```

### Error Handling
```rust
impl ActorGuest for Component {
    fn handle(evt: Event, state: Json) -> Json {
        match handle_event(evt, &state) {
            Ok(new_state) => new_state,
            Err(e) => {
                log(&format!("Error handling event: {}", e));
                // Return unchanged state on error
                state
            }
        }
    }
}
```

### Communication with Other Actors
```rust
use bindings::ntwk::theater::runtime::send;

// Send message to another actor
let message = json!({
    "type": "update",
    "data": { "value": 42 }
});

send("other-actor", &message);
```

## Best Practices

1. **State Management**
   - Keep state serializable
   - Use strong typing
   - Handle missing fields
   - Include timestamps

2. **Error Handling**
   - Log errors with context
   - Return unchanged state on error
   - Validate inputs
   - Handle all cases

3. **HTTP Handling**
   - Serve static files efficiently
   - Use appropriate status codes
   - Handle all routes
   - Validate request bodies

4. **Message Design**
   - Use clear message types
   - Include necessary context
   - Version messages if needed
   - Document message formats

5. **Testing**
   - Test state transitions
   - Verify error handling
   - Test HTTP endpoints
   - Check edge cases

## Development Tips

1. Start with a clear state structure
2. Use type-safe message handling
3. Implement error handling early
4. Test with real HTTP requests
5. Monitor actor logs

## Debugging

1. Use `log()` for visibility
2. Check state transitions
3. Verify message handling
4. Test HTTP endpoints
5. Monitor resource usage

The Theater actor model combines simplicity with power:
- JSON for state and messages
- HTTP integration
- File system access
- Strong typing
- Verifiable state
