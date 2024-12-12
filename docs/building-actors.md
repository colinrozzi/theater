# Building WebAssembly Component Actors: A Developer's Guide

## Introduction

WebAssembly components provide a secure, isolated foundation for building actor systems. This guide will walk you through creating actors that run in a WebAssembly sandbox, communicating through a well-defined message-passing interface.

## Project Structure

Create a new actor project with the following structure:

```
actor-name/
├── Cargo.toml
├── actor.toml        # Actor manifest file
├── src/
│   └── lib.rs
└── wit/
    └── actor.wit
```

## Actor Manifests

The actor manifest (`actor.toml`) is the central configuration file that defines:

### Basic Metadata
```toml
name = "my-actor"
version = "0.1.0"
description = "Does something awesome"
```

### Interfaces
```toml
[interfaces]
implements = ["ntwk:simple-actor/actor"]
requires = ["wasi:filesystem/files"]  # if you need filesystem access
```

### Capabilities
```toml
[capabilities.host]
filesystem = { access = ["read"], root = "./data" }
http = { bind_addr = "127.0.0.1:8080" }

[capabilities.actors]
logger = { interface = "ntwk:logging/logger" }
```

### Configuration
```toml
[config]
timeout_seconds = 30
max_retries = 3
allowed_origins = ["https://example.com"]
```

## Cargo Configuration

Your `Cargo.toml` should look like this:

```toml
[package]
name = "actor-name"
version = "0.1.0"
edition = "2021"

[dependencies]
serde_json = "1.0.133"
wit-bindgen-rt = { version = "0.35.0", features = ["bitflags"] }

[lib]
crate-type = ["cdylib"]

[profile.release]
codegen-units = 1
opt-level = "s"
debug = false
strip = true
lto = true

[package.metadata.component]
package = "ntwk:simple-actor"
```

## Actor Implementation

Here's the basic structure of an actor implementation:

```rust
mod bindings;

use bindings::exports::ntwk::simple_actor::actor::Guest;
use bindings::exports::ntwk::simple_actor::actor::Message;
use bindings::exports::ntwk::simple_actor::actor::State;

use bindings::ntwk::simple_actor::runtime::log;
use bindings::ntwk::simple_actor::runtime::send;

struct Component;

// Helper functions for state management
fn parse_json(data: &[u8]) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let str_data = String::from_utf8(data.to_vec())?;
    let json = serde_json::from_str(&str_data)?;
    Ok(json)
}

impl Guest for Component {
    fn init() -> Vec<u8> {
        log("Initializing actor");
        // Return initial state as JSON bytes
        serde_json::to_vec(&serde_json::json!({
            "state_field": "initial_value"
        })).unwrap()
    }

    fn handle(msg: Message, state: State) -> State {
        log("Processing message");
        let mut new_state = state.clone();

        // Process message and update state
        let result = parse_json(&msg).and_then(|msg_json| {
            match msg_json.get("action").and_then(|a| a.as_str()) {
                Some("some_action") => {
                    // Handle the action
                    // Update new_state accordingly
                    Ok(())
                }
                _ => Ok(())
            }
        });

        if let Err(e) = result {
            log(&format!("Error processing message: {}", e));
        }

        new_state
    }

    fn state_contract(state: State) -> bool {
        if let Ok(state_str) = String::from_utf8(state.clone()) {
            // Verify state is valid JSON
            if let Ok(state_json) = serde_json::from_str::<serde_json::Value>(&state_str) {
                // Add any additional state validation here
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    fn message_contract(msg: Message, _state: State) -> bool {
        if let Ok(msg_str) = String::from_utf8(msg.clone()) {
            // Verify message is valid JSON and has expected structure
            if let Ok(msg_json) = serde_json::from_str::<serde_json::Value>(&msg_str) {
                msg_json.get("action").is_some()
            } else {
                false
            }
        } else {
            false
        }
    }
}

bindings::export!(Component with_types_in bindings);
```

## State Management

States in this system are represented as byte vectors (`Vec<u8>`), typically containing JSON-encoded data. This approach provides flexibility while maintaining a simple interface.

```rust
// Helper function to get a field from state
fn get_field(state_json: &serde_json::Value, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    let value = state_json
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or(format!("{} not found or not a string", field))?;
    Ok(value.to_string())
}

// Helper function to update state
fn update_state(state: &[u8], updates: serde_json::Value) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut state_json = parse_json(state)?;
    
    if let serde_json::Value::Object(ref mut map) = state_json {
        if let serde_json::Value::Object(updates) = updates {
            for (k, v) in updates {
                map.insert(k, v);
            }
        }
    }
    
    Ok(serde_json::to_vec(&state_json)?)
}
```

## Message Handling

Messages, like state, are passed as byte vectors containing JSON data. Here's a typical message handling pattern:

```rust
fn handle(msg: Message, state: State) -> State {
    let result = parse_json(&msg).and_then(|msg_json| {
        match msg_json.get("action").and_then(|a| a.as_str()) {
            Some(action) => {
                match action {
                    "update_field" => {
                        let new_value = msg_json.get("value")
                            .ok_or("missing value field")?;
                        
                        let updates = serde_json::json!({
                            "field": new_value
                        });
                        
                        update_state(&state, updates)
                    }
                    _ => Ok(state.clone())
                }
            }
            None => Ok(state.clone())
        }
    });

    match result {
        Ok(new_state) => new_state,
        Err(e) => {
            log(&format!("Error: {}", e));
            state
        }
    }
}
```

## Contracts and Validation

Contract functions serve as guards to ensure state and message validity:

```rust
fn state_contract(state: State) -> bool {
    match parse_json(&state) {
        Ok(json) => {
            // Validate required fields exist
            let valid = json.get("required_field").is_some();
            // Add additional validation as needed
            valid
        }
        Err(_) => false
    }
}

fn message_contract(msg: Message, state: State) -> bool {
    match (parse_json(&msg), parse_json(&state)) {
        (Ok(msg_json), Ok(state_json)) => {
            // Validate message structure
            let has_action = msg_json.get("action").is_some();
            
            // Validate message is valid for current state
            let state_allows_action = true; // Add your logic here
            
            has_action && state_allows_action
        }
        _ => false
    }
}
```

## Building and Testing

### Building
```bash
cargo component build --target wasm32-unknown-unknown --release
```

### Testing
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
    fn test_message_handling() {
        let state = Component::init();
        let msg = serde_json::to_vec(&serde_json::json!({
            "action": "test_action"
        })).unwrap();
        
        assert!(Component::message_contract(msg.clone(), state.clone()));
        let new_state = Component::handle(msg, state);
        assert!(Component::state_contract(new_state));
    }
}
```

## Runtime Integration

### Configuration Access
```rust
use bindings::ntwk::simple_actor::runtime::get_config;

fn init() -> Vec<u8> {
    // Get configuration from manifest
    let config = get_config().expect("Failed to get config");
    
    // Use configuration values
    let timeout = config["timeout_seconds"]
        .as_u64()
        .expect("Missing timeout config");
        
    // Initialize state with config
    serde_json::to_vec(&serde_json::json!({
        "timeout": timeout,
        "status": "initialized"
    })).unwrap()
}
```

### Runtime Communication
```rust
// Logging
log(&format!("Processing action: {}", action));

// Send message to another actor
let message = serde_json::to_vec(&serde_json::json!({
    "action": "notify",
    "data": "something changed"
})).unwrap();
send("other-actor", message);
```

## Best Practices

1. **State Management**
   - Keep states minimal and focused
   - Validate all state transitions
   - Use clear field names
   - Document state structure

2. **Message Design**
   - Use clear action names
   - Include necessary data only
   - Version message formats if needed
   - Document message structure

3. **Error Handling**
   - Log meaningful errors
   - Return to safe states on failure
   - Validate all inputs
   - Handle all error cases

4. **Testing**
   - Test all message handlers
   - Verify state transitions
   - Test contract functions
   - Include edge cases

## Common Patterns

### State Machine
```rust
#[derive(Serialize, Deserialize)]
enum State {
    Initial,
    Processing { started_at: String },
    Complete { result: String },
    Error { message: String }
}

fn handle_state_transition(current: State, action: Action) -> State {
    match (current, action) {
        (State::Initial, Action::Start) => State::Processing {
            started_at: Utc::now().to_rfc3339()
        },
        (State::Processing { .. }, Action::Complete(result)) => State::Complete {
            result
        },
        // ... other transitions
        _ => current
    }
}
```

### Event Sourcing
```rust
#[derive(Serialize, Deserialize)]
enum Event {
    Created { id: String, timestamp: String },
    Updated { field: String, value: String },
    Completed { timestamp: String }
}

fn apply_event(state: &mut Value, event: Event) {
    match event {
        Event::Created { id, timestamp } => {
            state["id"] = json!(id);
            state["created_at"] = json!(timestamp);
        },
        Event::Updated { field, value } => {
            state[field] = json!(value);
        },
        Event::Completed { timestamp } => {
            state["completed_at"] = json!(timestamp);
        }
    }
}
```

## Deployment

1. Build your component:
```bash
cargo component build --target wasm32-unknown-unknown --release
```

2. Create your manifest file (actor.toml)

3. Deploy using the theater runtime:
```bash
theater-cli deploy ./actor.toml
```

4. Monitor your actor:
```bash
theater-cli status <actor-id>
```

Remember to regularly check the chain state and monitor your actor's health when deployed in production.