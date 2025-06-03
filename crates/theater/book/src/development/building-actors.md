# Building Actors in Theater

This guide walks you through creating actors in Theater, from basic concepts to advanced patterns, with practical examples.

## Quick Start

Create a new actor project:

```bash
cargo new my-actor
cd my-actor
```

Add dependencies to Cargo.toml:
```toml
[package]
name = "my-actor"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }
```

## Project Structure

```
my-actor/
├── Cargo.toml              # Project configuration
├── actor.toml             # Actor manifest
├── src/
│   ├── lib.rs            # Actor implementation
│   └── state.rs          # State management
└── wit/                  # Interface definitions
    └── actor.wit         # Actor interface
```

## Basic Actor Implementation

Here's a complete example of a simple counter actor:

```rust
// src/lib.rs
use bindings::exports::ntwk::theater::actor::Guest as ActorGuest;
use bindings::ntwk::theater::types::{Event, Json};
use bindings::ntwk::theater::runtime::log;
use serde::{Deserialize, Serialize};

// Define actor state
#[derive(Serialize, Deserialize)]
struct State {
    count: i32,
    last_updated: String,
}

// Define message types
#[derive(Deserialize)]
#[serde(tag = "type")]
enum Message {
    Increment { amount: i32 },
    Decrement { amount: i32 },
    Reset,
}

struct Component;

impl ActorGuest for Component {
    fn init() -> Vec<u8> {
        log("Initializing counter actor");
        
        let initial_state = State {
            count: 0,
            last_updated: chrono::Utc::now().to_string(),
        };
        
        serde_json::to_vec(&initial_state).unwrap()
    }

    fn handle(evt: Event, state: Vec<u8>) -> Vec<u8> {
        log(&format!("Handling event: {:?}", evt));
        
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        
        if let Ok(message) = serde_json::from_slice(&evt.data) {
            match message {
                Message::Increment { amount } => {
                    current_state.count += amount;
                }
                Message::Decrement { amount } => {
                    current_state.count -= amount;
                }
                Message::Reset => {
                    current_state.count = 0;
                }
            }
            current_state.last_updated = chrono::Utc::now().to_string();
        }
        
        serde_json::to_vec(&current_state).unwrap()
    }
}

bindings::export!(Component with_types_in bindings);
```

## Actor Manifest

Configure your actor in actor.toml:

```toml
name = "counter-actor"
component_path = "target/wasm32-wasi/release/counter_actor.wasm"

[interface]
implements = "theater:simple/actor"
requires = []

[[handlers]]
type = "http-server"
config = { port = 8080 }

[logging]
level = "debug"
output = "stdout"
```

## Adding HTTP Capabilities

Extend the actor to handle HTTP requests:

```rust
use bindings::exports::ntwk::theater::http_server::Guest as HttpGuest;
use bindings::ntwk::theater::http_server::{HttpRequest, HttpResponse};

impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        match (req.method.as_str(), req.path.as_str()) {
            // Get current count
            ("GET", "/count") => {
                let current_state: State = serde_json::from_slice(&state).unwrap();
                
                (HttpResponse {
                    status: 200,
                    headers: vec![
                        ("Content-Type".to_string(), "application/json".to_string())
                    ],
                    body: Some(serde_json::json!({
                        "count": current_state.count,
                        "last_updated": current_state.last_updated
                    }).to_string().into_bytes()),
                }, state)
            },
            
            // Increment count
            ("POST", "/increment") => {
                if let Some(body) = req.body {
                    if let Ok(increment) = serde_json::from_slice::<serde_json::Value>(&body) {
                        let amount = increment["amount"].as_i64().unwrap_or(1) as i32;
                        
                        let evt = Event {
                            event_type: "increment".to_string(),
                            parent: None,
                            data: serde_json::json!({
                                "type": "Increment",
                                "amount": amount
                            }).to_string().into_bytes(),
                        };
                        
                        let new_state = Component::handle(evt, state);
                        
                        return (HttpResponse {
                            status: 200,
                            headers: vec![
                                ("Content-Type".to_string(), "application/json".to_string())
                            ],
                            body: Some(b"{"status":"ok"}".to_vec()),
                        }, new_state);
                    }
                }
                
                (HttpResponse {
                    status: 400,
                    headers: vec![],
                    body: Some(b"{"error":"invalid request"}".to_vec()),
                }, state)
            },
            
            _ => (HttpResponse {
                status: 404,
                headers: vec![],
                body: None,
            }, state)
        }
    }
}
```

## Adding WebSocket Support

Enable real-time updates with WebSocket support:

```rust
use bindings::exports::ntwk::theater::websocket_server::Guest as WebSocketGuest;
use bindings::ntwk::theater::websocket_server::{
    WebSocketMessage,
    WebSocketResponse,
    MessageType
};

impl WebSocketGuest for Component {
    fn handle_message(msg: WebSocketMessage, state: Json) -> (Json, WebSocketResponse) {
        match msg.ty {
            MessageType::Text => {
                if let Some(text) = msg.text {
                    // Parse command
                    if let Ok(command) = serde_json::from_str::<serde_json::Value>(&text) {
                        match command["action"].as_str() {
                            Some("subscribe") => {
                                // Send current state
                                let current_state: State = 
                                    serde_json::from_slice(&state).unwrap();
                                    
                                return (state, WebSocketResponse {
                                    messages: vec![WebSocketMessage {
                                        ty: MessageType::Text,
                                        text: Some(serde_json::json!({
                                            "type": "update",
                                            "count": current_state.count
                                        }).to_string()),
                                        data: None,
                                    }]
                                });
                            },
                            _ => {}
                        }
                    }
                }
            },
            _ => {}
        }
        
        (state, WebSocketResponse { messages: vec![] })
    }
}
```

## Using Host Functions

Theater provides several host functions for common operations:

```rust
use bindings::ntwk::theater::runtime::{log, spawn};
use bindings::ntwk::theater::filesystem::read_file;

// Logging
log("Actor processing message...");

// Spawn another actor
spawn("other-actor.toml");

// Read a file
let content = read_file("config.json");
```

## State Management Best Practices

1. **Use Strong Typing**
```rust
#[derive(Serialize, Deserialize)]
struct State {
    data: HashMap<String, Value>,
    metadata: Metadata,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    version: u32,
    owner: String,
}
```

2. **Handle Errors Gracefully**
```rust
fn handle(evt: Event, state: Json) -> Json {
    let current_state: State = match serde_json::from_slice(&state) {
        Ok(state) => state,
        Err(e) => {
            log(&format!("Error parsing state: {}", e));
            return state; // Return unchanged state on error
        }
    };
    
    // Process event...
}
```

3. **Include Timestamps**
```rust
fn update_state(mut state: State) -> State {
    state.updated_at = chrono::Utc::now();
    state
}
```

## Testing

Create tests for your actor:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_increment() {
        let state = State {
            count: 0,
            last_updated: chrono::Utc::now().to_string(),
        };
        
        let event = Event {
            event_type: "increment".to_string(),
            parent: None,
            data: serde_json::json!({
                "type": "Increment",
                "amount": 5
            }).to_string().into_bytes(),
        };
        
        let state_json = serde_json::to_vec(&state).unwrap();
        let new_state_json = Component::handle(event, state_json);
        let new_state: State = serde_json::from_slice(&new_state_json).unwrap();
        
        assert_eq!(new_state.count, 5);
    }
}
```

## Advanced Patterns

### 1. State History
```rust
#[derive(Serialize, Deserialize)]
struct State {
    current: StateData,
    history: VecDeque<StateChange>,
}

#[derive(Serialize, Deserialize)]
struct StateChange {
    timestamp: DateTime<Utc>,
    change_type: String,
    previous_value: Value,
}
```

### 2. Event Correlation
```rust
#[derive(Serialize, Deserialize)]
struct Event {
    id: String,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    data: Value,
}
```

### 3. Validation Chain
```rust
fn validate_state(state: &State) -> Result<(), String> {
    validate_constraints(state)?;
    validate_relationships(state)?;
    validate_business_rules(state)?;
    Ok(())
}
```

## Development Tips

1. Use the runtime log function liberally
2. Test with different message types
3. Verify state transitions
4. Handle all error cases
5. Monitor the hash chain
6. Test all handler interfaces

## Common Pitfalls

1. **Not Handling JSON Errors**
   - Always handle deserialization errors
   - Validate JSON structure
   - Handle missing fields

2. **State Inconsistency**
   - Validate state after changes
   - Keep state updates atomic
   - Handle partial updates

3. **Missing Error Logging**
   - Log all errors
   - Include context
   - Track error patterns

4. **Resource Management**
   - Clean up resources
   - Handle timeouts
   - Monitor memory usage