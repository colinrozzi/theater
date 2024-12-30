# Building WebAssembly Component Actors

## Project Structure
```
actor-name/
├── Cargo.toml
├── actor.toml        # Actor manifest
├── src/
│   └── lib.rs        # Actor implementation
└── wit/
    └── actor.wit     # Interface definition
```

## Actor Manifest

### Basic Configuration
```toml
name = "my-actor"
component_path = "path/to/actor.wasm"

[interface]
implements = "ntwk:simple-actor/actor"
requires = []

[[handlers]]
type = "Http-server"
config = { port = 8080 }

[[handlers]]
type = "Message-server"
config = { port = 8081 }

[logging]
chain_events = true
level = "info"
output = "stdout"
```

### Handler Types
```toml
# HTTP Server
[[handlers]]
type = "Http-server"
config = { port = 8080 }

# Message Server
[[handlers]]
type = "Message-server"
config = { port = 8081 }
```

## Component Implementation

### Required Exports
```rust
// Initialize actor state
fn init() -> Vec<u8>;

// Handle incoming events
fn handle(
    msg: Vec<u8>, 
    state: Vec<u8>
) -> (Vec<u8>, Option<Vec<u8>>);

// Verify state validity
fn state_contract(state: Vec<u8>) -> bool;

// Verify message validity
fn message_contract(msg: Vec<u8>, state: Vec<u8>) -> bool;
```

### Available Host Functions
```rust
// Log messages
fn log(msg: &str);

// Send message to another actor
fn send(address: &str, msg: &[u8]);

// Send HTTP request (if HTTP capability enabled)
fn http_send(address: &str, msg: &[u8]) -> Vec<u8>;
```

## Implementation Example

```rust
use serde_json::{json, Value};

struct Component;

impl Guest for Component {
    fn init() -> Vec<u8> {
        let initial_state = json!({
            "counter": 0,
            "last_updated": null
        });
        
        serde_json::to_vec(&initial_state).unwrap()
    }

    fn handle(msg: Vec<u8>, state: Vec<u8>) -> (Vec<u8>, Option<Vec<u8>>) {
        let msg: Value = serde_json::from_slice(&msg).unwrap();
        let mut state: Value = serde_json::from_slice(&state).unwrap();

        match msg.get("type").and_then(|t| t.as_str()) {
            Some("increment") => {
                if let Some(counter) = state.get_mut("counter") {
                    *counter = json!(counter.as_i64().unwrap() + 1);
                }
                
                let response = json!({
                    "type": "increment_response",
                    "data": {
                        "new_value": state["counter"]
                    }
                });

                (
                    serde_json::to_vec(&state).unwrap(),
                    Some(serde_json::to_vec(&response).unwrap())
                )
            },
            _ => (state.to_vec(), None)
        }
    }

    fn state_contract(state: Vec<u8>) -> bool {
        if let Ok(state) = serde_json::from_slice::<Value>(&state) {
            state.get("counter").is_some() && 
            state.get("last_updated").is_some()
        } else {
            false
        }
    }

    fn message_contract(msg: Vec<u8>, _state: Vec<u8>) -> bool {
        if let Ok(msg) = serde_json::from_slice::<Value>(&msg) {
            msg.get("type").is_some()
        } else {
            false
        }
    }
}

bindings::export!(Component with_types_in bindings);
```

## State Management

### State Structure
- Use JSON for flexibility
- Keep states minimal
- Include all required fields
- Version if needed

### State Validation
```rust
fn validate_state(state: &Value) -> bool {
    state.get("counter").is_some() &&
    state.get("counter").unwrap().is_i64() &&
    state.get("last_updated").is_some()
}
```

### State Updates
```rust
fn update_state(mut state: Value, field: &str, value: Value) -> Value {
    if let Some(field_value) = state.get_mut(field) {
        *field_value = value;
    }
    state
}
```

## Event Handling

### Event Structure
```rust
struct Event {
    type_: String,
    data: Value,
}
```

### Event Processing
```rust
fn process_event(event: &Event, state: &mut Value) -> Option<Event> {
    match event.type_.as_str() {
        "update" => {
            // Update state
            Some(Event {
                type_: "update_complete".to_string(),
                data: json!({"status": "success"})
            })
        },
        _ => None
    }
}
```

## Testing

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let state = Component::init();
        assert!(Component::state_contract(state));
    }

    #[test]
    fn test_increment() {
        let state = Component::init();
        let msg = serde_json::to_vec(&json!({
            "type": "increment"
        })).unwrap();
        
        let (new_state, response) = Component::handle(msg, state);
        assert!(Component::state_contract(new_state));
        assert!(response.is_some());
    }
}
```

### Integration Tests
```rust
#[tokio::test]
async fn test_http_handler() {
    let actor = ActorRuntime::from_file("test-actor.toml").await.unwrap();
    
    let response = reqwest::Client::new()
        .post("http://localhost:8080")
        .json(&json!({
            "type": "increment"
        }))
        .send()
        .await
        .unwrap();
        
    assert!(response.status().is_success());
}
```

## Best Practices

1. **State Management**
   - Keep states minimal
   - Validate all transitions
   - Use clear field names
   - Version if needed

2. **Event Handling**
   - Clear event types
   - Proper error handling
   - Validate inputs
   - Return appropriate responses

3. **Testing**
   - Test all handlers
   - Verify state transitions
   - Test error cases
   - Integration tests

4. **Security**
   - Validate all inputs
   - Sanitize outputs
   - Handle errors gracefully
   - Log security events

## Deployment

1. Build the component:
```bash
cargo build --target wasm32-unknown-unknown --release
```

2. Deploy with theater:
```bash
theater-cli deploy actor.toml
```

3. Monitor logs:
```bash
theater-cli logs <actor-id>
```

4. Check status:
```bash
theater-cli status <actor-id>
```

## Troubleshooting

1. **Build Issues**
   - Check wasm target is installed
   - Verify dependencies
   - Check component interface

2. **Runtime Issues**
   - Check logs
   - Verify manifest
   - Check port availability
   - Validate handlers

3. **State Issues**
   - Verify state contract
   - Check transitions
   - Validate JSON
   - Check chain integrity