# Theater API Reference

## Core Actor Interface

The fundamental interface all Theater actors implement:

```rust
/// Core actor trait that all Theater actors must implement
pub trait Actor {
    /// Handle an incoming message and current state, return new state and response
    /// 
    /// # Arguments
    /// * `state` - Current actor state as JSON string
    /// * `message` - Incoming message as JSON string
    /// 
    /// # Returns
    /// Tuple of (new_state, response) as JSON strings
    fn handle(&self, state: &str, message: &str) -> Result<(String, String), ActorError>;
}

/// Common error types for actor operations
#[derive(Debug)]
pub enum ActorError {
    /// Invalid JSON in state or message
    InvalidJson(String),
    /// Error processing message
    ProcessingError(String),
    /// State validation failed
    StateError(String),
}
```

### Example Implementation

```rust
use serde_json::{json, Value};
use theater::Actor;

struct CounterActor;

impl Actor for CounterActor {
    fn handle(&self, state: &str, message: &str) -> Result<(String, String), ActorError> {
        // Parse current state
        let mut state: Value = serde_json::from_str(state)
            .map_err(|e| ActorError::InvalidJson(e.to_string()))?;
        
        // Parse message
        let message: Value = serde_json::from_str(message)
            .map_err(|e| ActorError::InvalidJson(e.to_string()))?;

        // Handle increment message
        if message["type"] == "increment" {
            let amount = message["amount"]
                .as_i64()
                .ok_or(ActorError::ProcessingError("Invalid amount".into()))?;
            
            let new_count = state["count"].as_i64().unwrap_or(0) + amount;
            state["count"] = json!(new_count);
            
            // Return new state and response
            Ok((
                state.to_string(),
                json!({
                    "type": "increment_complete",
                    "new_count": new_count
                }).to_string()
            ))
        } else {
            Err(ActorError::ProcessingError("Unknown message type".into()))
        }
    }
}
```

## HTTP Handler Interface

Interface for actors that handle HTTP requests:

```rust
/// HTTP handler trait for actors that serve HTTP requests
pub trait HttpHandler {
    /// Handle an HTTP request
    /// 
    /// # Arguments
    /// * `request` - The incoming HTTP request
    /// * `state` - Current actor state
    /// 
    /// # Returns
    /// HTTP response and new state
    fn handle_request(
        &self,
        request: HttpRequest,
        state: &str
    ) -> Result<(HttpResponse, String), ActorError>;
}

/// HTTP request structure
#[derive(Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// HTTP response structure
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
```

### Example HTTP Handler

```rust
impl HttpHandler for CounterActor {
    fn handle_request(
        &self,
        request: HttpRequest,
        state: &str,
    ) -> Result<(HttpResponse, String), ActorError> {
        match (request.method.as_str(), request.path.as_str()) {
            // GET /count - Return current count
            ("GET", "/count") => {
                let state: Value = serde_json::from_str(state)?;
                let count = state["count"].as_i64().unwrap_or(0);
                
                Ok((
                    HttpResponse {
                        status: 200,
                        headers: HashMap::from([
                            ("Content-Type".into(), "application/json".into())
                        ]),
                        body: json!({ "count": count }).to_string().into_bytes(),
                    },
                    state.to_string()
                ))
            },
            
            // POST /increment - Increment counter
            ("POST", "/increment") => {
                let body: Value = serde_json::from_slice(&request.body.unwrap_or_default())?;
                
                // Reuse actor message handling
                let (new_state, response) = self.handle(
                    state,
                    json!({
                        "type": "increment",
                        "amount": body["amount"]
                    }).to_string().as_str()
                )?;
                
                Ok((
                    HttpResponse {
                        status: 200,
                        headers: HashMap::from([
                            ("Content-Type".into(), "application/json".into())
                        ]),
                        body: response.into_bytes(),
                    },
                    new_state
                ))
            },
            
            // 404 for unknown routes
            _ => Ok((
                HttpResponse {
                    status: 404,
                    headers: HashMap::new(),
                    body: vec![],
                },
                state.to_string()
            )),
        }
    }
}
```

## Supervision API

Interface for actors that supervise other actors:

```rust
/// Supervisor trait for managing child actors
pub trait Supervisor {
    /// Handle lifecycle events from supervised actors
    fn handle_lifecycle(
        &self,
        event: LifecycleEvent,
        state: &str
    ) -> Result<(String, SupervisorAction), ActorError>;
}

/// Events from supervised actors
#[derive(Debug)]
pub enum LifecycleEvent {
    Started {
        actor_id: String,
        initial_state: String,
    },
    Stopped {
        actor_id: String,
        final_state: String,
    },
    Failed {
        actor_id: String,
        error: String,
        state: String,
    },
}

/// Actions a supervisor can take
#[derive(Debug)]
pub enum SupervisorAction {
    /// Do nothing
    Continue,
    /// Restart the actor
    Restart {
        actor_id: String,
        initial_state: String,
    },
    /// Stop the actor
    Stop {
        actor_id: String,
    },
    /// Escalate the failure
    Escalate {
        error: String,
    },
}
```

### Example Supervisor

```rust
impl Supervisor for WorkerSupervisor {
    fn handle_lifecycle(
        &self,
        event: LifecycleEvent,
        state: &str
    ) -> Result<(String, SupervisorAction), ActorError> {
        let mut state: Value = serde_json::from_str(state)?;
        
        match event {
            LifecycleEvent::Failed { actor_id, error, state: failed_state } => {
                // Track failure in supervisor state
                let failures = state["failures"]
                    .as_array_mut()
                    .ok_or(ActorError::StateError("Invalid failures array".into()))?;
                
                failures.push(json!({
                    "actor_id": actor_id,
                    "error": error,
                    "timestamp": chrono::Utc::now(),
                    "state": failed_state
                }));
                
                // Decide whether to restart
                if failures.len() < 3 {
                    Ok((
                        state.to_string(),
                        SupervisorAction::Restart {
                            actor_id,
                            initial_state: json!({ "count": 0 }).to_string(),
                        }
                    ))
                } else {
                    Ok((
                        state.to_string(),
                        SupervisorAction::Escalate {
                            error: "Too many failures".into()
                        }
                    ))
                }
            },
            
            // Handle other lifecycle events...
            _ => Ok((state.to_string(), SupervisorAction::Continue))
        }
    }
}
```

## Hash Chain API

Interface for working with state hash chains:

```rust
/// Hash chain operations
pub trait HashChain {
    /// Get entry by hash
    fn get_entry(&self, hash: &str) -> Result<HashEntry, HashChainError>;
    
    /// Get latest entry
    fn get_latest(&self) -> Result<HashEntry, HashChainError>;
    
    /// Verify entry and its history
    fn verify(&self, hash: &str) -> Result<bool, HashChainError>;
    
    /// Get history between hashes
    fn get_history(
        &self,
        start_hash: &str,
        end_hash: &str
    ) -> Result<Vec<HashEntry>, HashChainError>;
}

/// Entry in the hash chain
#[derive(Debug)]
pub struct HashEntry {
    pub hash: String,
    pub parent_hash: Option<String>,
    pub state: String,
    pub message: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}
```

### Example Hash Chain Usage

```rust
use theater::HashChain;

fn verify_state_history(chain: &impl HashChain, hash: &str) -> Result<(), HashChainError> {
    // Get entry to verify
    let entry = chain.get_entry(hash)?;
    
    // Verify this entry and its history
    if !chain.verify(hash)? {
        return Err(HashChainError::VerificationFailed);
    }
    
    // Get history from start to this entry
    let history = chain.get_history(
        &chain.get_latest()?.hash,
        hash
    )?;
    
    // Process history
    for entry in history {
        println!(
            "State transition at {}: {} -> {}",
            entry.timestamp,
            entry.parent_hash.unwrap_or_else(|| "none".into()),
            entry.hash
        );
    }
    
    Ok(())
}
```

## Configuration Types

Common configuration structures:

```rust
/// Actor configuration in manifest
#[derive(Debug, Deserialize)]
pub struct ActorConfig {
    pub name: String,
    pub component_path: String,
    pub interface: InterfaceConfig,
    pub handlers: Vec<HandlerConfig>,
}

/// Interface implementation configuration
#[derive(Debug, Deserialize)]
pub struct InterfaceConfig {
    pub implements: Vec<String>,
    pub requires: Vec<String>,
}

/// Handler configuration
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum HandlerConfig {
    #[serde(rename = "Http-server")]
    HttpServer {
        config: HttpServerConfig,
    },
    #[serde(rename = "Http-client")]
    HttpClient {
        config: HttpClientConfig,
    },
    #[serde(rename = "Metrics")]
    Metrics {
        config: MetricsConfig,
    },
}

/// HTTP server configuration
#[derive(Debug, Deserialize)]
pub struct HttpServerConfig {
    pub port: u16,
    pub host: Option<String>,
    pub tls: Option<TlsConfig>,
}
```

## Error Types

Common error types used across the API:

```rust
/// Actor-related errors
#[derive(Debug)]
pub enum ActorError {
    InvalidJson(String),
    ProcessingError(String),
    StateError(String),
    InterfaceError(String),
}

/// Hash chain errors
#[derive(Debug)]
pub enum HashChainError {
    EntryNotFound(String),
    InvalidHash(String),
    VerificationFailed,
    DatabaseError(String),
}

/// Handler errors
#[derive(Debug)]
pub enum HandlerError {
    Configuration(String),
    Runtime(String),
    Protocol(String),
}
```

## Best Practices

1. **Error Handling**
   - Use specific error types
   - Include context in errors
   - Handle all error cases
   - Maintain state consistency

2. **State Management**
   - Validate state transitions
   - Keep state serializable
   - Use appropriate types
   - Handle missing fields

3. **Message Processing**
   - Validate message format
   - Handle unknown messages
   - Check field types
   - Use typed structures

4. **HTTP Handling**
   - Validate routes
   - Check content types
   - Handle missing data
   - Use status codes properly

5. **Supervision**
   - Track failure history
   - Use appropriate restart strategy
   - Handle all lifecycle events
   - Maintain supervisor state