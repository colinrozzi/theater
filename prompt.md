Hello! I am working on a project and I am running into some issue I cannot figure out. Right now, I am working on the http.rs file, and specifically I am working on setting up the handler for the post request, but I have included the full project for reference.

<project>

<file doc.md>
# Runtime V2

## Overview
Runtime V2 is a redesigned actor system that enables state management, verification, and flexible interaction patterns for WebAssembly components. The system is built around a core concept: actors that maintain verifiable state and can interact with the outside world in various ways.

## Key Concepts

### Actors
An actor in this system is a WebAssembly component that:
- Maintains state
- Responds to inputs by producing outputs and updating state
- Participates in a verifiable hash chain of all state changes
- Can interact with the outside world through various interfaces

### Hash Chain
All actor state changes are recorded in a verifiable hash chain. This enables:
- Complete history of how an actor reached its current state
- Verification of state transitions
- Ability to replay and audit state changes
- Cross-actor state verification

### Flexible Interfaces
The system is designed to support multiple ways for actors to interact with the outside world:
- Message passing between actors
- HTTP server capabilities
- Future interfaces (filesystem, timers, etc.)

## Core Architecture

### ActorInput and ActorOutput
These enums represent all possible ways data can flow into and out of an actor:
```rust
pub enum ActorInput {
    Message(Value),
    HttpRequest { ... },
    // Future input types
}

pub enum ActorOutput {
    Message(Value),
    HttpResponse { ... },
    // Future output types
}
```

This design:
- Makes all possible interactions explicit
- Enables type-safe handling of different interaction patterns
- Allows easy addition of new interaction types
- Ensures consistent chain recording of all inputs

### Actor Trait
The core interface that all actors must implement:
```rust
pub trait Actor {
    fn init(&self) -> Result<Value>;
    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)>;
    fn verify_state(&self, state: &Value) -> bool;
}
```

Key design decisions:
- Use of serde_json::Value for state enables flexible state representation
- Single handle_input method unifies all interaction types
- Explicit state verification support
- Clear initialization pattern

### ActorRuntime
Manages the core actor lifecycle:
- State management
- Chain recording
- Input handling
- State verification

### Interfaces
The ActorInterface trait enables multiple ways to expose actors:
```rust
pub trait ActorInterface {
    type Config;
    fn new(config: Self::Config) -> Result<Self> where Self: Sized;
    fn start(self, runtime: ActorRuntime<impl Actor>) -> Result<()>;
}
```

This allows:
- Clean separation between core actor logic and exposure mechanisms
- Multiple simultaneous interfaces per actor
- Easy addition of new interface types

## Roadmap

### Phase 1: Core Implementation
1. Complete basic message-passing interface
   - Implement MessageInterface
   - Port existing actor-to-actor communication
   - Add tests for basic messaging

2. Add WASM component integration
   - Create WasmActor implementation
   - Add manifest parsing
   - Implement host functions
   - Test with simple components

### Phase 2: HTTP Support
1. Implement HttpInterface
   - HTTP server setup
   - Request/response handling
   - Chain recording for HTTP interactions

2. Create HTTP actor examples
   - Simple static file server
   - REST API example
   - WebSocket support investigation

### Phase 3: Enhanced Features
1. Add more interface types
   - Filesystem access
   - Timer/scheduling
   - Database connections

2. Improve chain verification
   - Cross-actor verification
   - Chain pruning strategies
   - Performance optimizations

3. Development tools
   - Chain visualization
   - Actor debugging tools
   - State inspection utilities

## Contributing
When adding new features:
1. Consider how they fit into the core abstractions
2. Ensure all state changes are properly recorded
3. Add appropriate tests
4. Update documentation

## Design Principles
1. **Explicit over implicit**: All possible interactions should be explicitly modeled in the type system.
2. **Verifiable state**: Every state change must be recorded and verifiable.
3. **Extensible interfaces**: New ways of interacting with actors should be easy to add.
4. **Clean separation**: Core actor logic should be separate from interface mechanisms.
5. **Type safety**: Use the type system to prevent invalid interactions.

## Development Setup
[To be added: Development environment setup, build instructions, test running]
</file>
<file lib.rs>
use anyhow::Result;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

mod chain;
mod http;
mod wasm;

pub use wasm::{WasmActor, WasmError};

// Core types that represent different kinds of actor interactions
#[derive(Debug, Clone)]
pub enum ActorInput {
    /// Regular actor-to-actor messages
    Message(Value),

    /// HTTP requests
    HttpRequest {
        method: String,
        uri: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
    // Future input types go here
}

#[derive(Debug, Clone)]
pub enum ActorOutput {
    /// Regular actor-to-actor messages
    Message(Value),

    /// HTTP responses
    HttpResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
    // Future output types go here
}

pub struct ActorMessage {
    pub content: ActorInput,
    pub response_channel: Option<oneshot::Sender<ActorOutput>>,
}

/// Core trait that all actors must implement
pub trait Actor {
    /// Initialize the actor and return its initial state
    fn init(&self) -> Result<Value>;

    /// Handle an input and return the output along with the new state
    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)>;

    /// Verify that a given state is valid for this actor
    fn verify_state(&self, state: &Value) -> bool;
}

/// The core actor process that handles messages
pub struct ActorProcess {
    mailbox_rx: mpsc::Receiver<ActorMessage>,
    state: Value,
    chain: chain::HashChain,
    actor: Box<dyn Actor>,
}

impl ActorProcess {
    pub fn new(
        actor: Box<dyn Actor>,
        mailbox_rx: mpsc::Receiver<ActorMessage>,
    ) -> Result<Self> {
        let mut chain = chain::HashChain::new();
        chain.add(Value::Null); // Initialize chain with null entry

        let state = actor.init()?;
        chain.add(state.clone());

        Ok(Self {
            mailbox_rx,
            state,
            chain,
            actor,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Some(msg) = self.mailbox_rx.recv().await {
            let (output, new_state) = self.actor.handle_input(msg.content, &self.state)?;
            
            // Update state and chain
            self.state = new_state.clone();
            self.chain.add(new_state);

            // Send response if channel exists
            if let Some(response_tx) = msg.response_channel {
                // Ignore error if receiver was dropped
                let _ = response_tx.send(output);
            }
        }

        Ok(())
    }

    pub fn get_chain(&self) -> &chain::HashChain {
        &self.chain
    }
}

/// Trait for different ways of exposing actors to the world
pub trait HostInterface: Send + Sync {
    async fn start(
        &mut self,
        mailbox_tx: mpsc::Sender<ActorMessage>
    ) -> Result<()>;
    
    async fn stop(&mut self) -> Result<()>;
}

// Configuration options for host interfaces
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub port: u16,
}

// Represents a host interface type in the manifest
#[derive(Debug, Clone)]
pub enum HostInterfaceType {
    Http(HttpConfig),
    // Add more interface types here
}

// Manifest configuration
#[derive(Debug, Clone)]
pub struct ActorConfig {
    pub name: String,
    pub component_path: String,
    pub interfaces: Vec<HostInterfaceType>,
}
</file>
<file http.rs>
use anyhow::Result;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use axum_macros::debug_handler;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::{ActorInput, ActorMessage, ActorOutput, HostInterface};

pub struct HttpHost {
    port: u16,
    mailbox_tx: Option<mpsc::Sender<ActorMessage>>,
}

impl HttpHost {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            mailbox_tx: None,
        }
    }

    #[debug_handler]
    async fn handle_request(
        State(mailbox_tx): State<mpsc::Sender<ActorMessage>>,
        Json(payload): Json<Value>,
    ) -> Result<Response, StatusCode> {
        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Send message to actor
        let msg = ActorMessage {
            content: ActorInput::Message(payload),
            response_channel: Some(tx),
        };

        mailbox_tx
            .send(msg)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        match response {
            ActorOutput::Message(value) => Ok(Json(value).into_response()),
            ActorOutput::HttpResponse { .. } => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl HostInterface for HttpHost {
    async fn start(&mut self, mailbox_tx: mpsc::Sender<ActorMessage>) -> Result<()> {
        self.mailbox_tx = Some(mailbox_tx.clone());

        // Build router
        let app = Router::new()
            .route("/", post(HttpHost::handle_request))
            .with_state(mailbox_tx.clone());

        // Run server
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
        println!("HTTP interface listening on http://{}", addr);

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        // Server will stop when dropped
        Ok(())
    }
}
</file>
<file chain.rs>
use md5;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    pub parent: Option<String>,
    pub data: Value,
}

#[derive(Debug)]
pub struct HashChain {
    head: Option<String>,
    entries: HashMap<String, ChainEntry>,
}

impl HashChain {
    pub fn new() -> Self {
        Self {
            head: None,
            entries: HashMap::new(),
        }
    }

    pub fn add(&mut self, data: Value) -> String {
        let entry = ChainEntry {
            parent: self.head.clone(),
            data,
        };

        // Calculate hash of entry
        let serialized = serde_json::to_string(&entry).expect("Failed to serialize entry");
        let hash = format!("{:x}", md5::compute(serialized));

        // Store entry and update head
        self.entries.insert(hash.clone(), entry);
        self.head = Some(hash.clone());

        hash
    }

    pub fn get_head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    pub fn get_full_chain(&self) -> Vec<(String, ChainEntry)> {
        let mut result = Vec::new();
        let mut current = self.head.clone();

        while let Some(hash) = current {
            let entry = self.entries.get(&hash).expect("Chain corrupted").clone();
            result.push((hash.clone(), entry.clone()));
            current = entry.parent;
        }

        result
    }
}
</file>
<file wasm.rs>
use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use thiserror::Error;
use wasmtime::component::ComponentExportIndex;
use wasmtime::component::{Component, Instance, Linker};
use wasmtime::{Engine, Store};

use crate::{Actor, ActorInput, ActorOutput};

#[derive(Error, Debug)]
pub enum WasmError {
    #[error("Failed to load manifest: {0}")]
    ManifestError(String),

    #[error("WASM error: {context} - {message}")]
    WasmError {
        context: &'static str,
        message: String,
    },
}

/// Implementation of the Actor trait for WebAssembly components
pub struct WasmActor {
    engine: Engine,
    component: Component,
    linker: Linker<()>,
    init_index: ComponentExportIndex,
    handle_index: ComponentExportIndex,
    state_contract_index: ComponentExportIndex,
    message_contract_index: ComponentExportIndex,
    // Optional indices for HTTP support
    http_contract_index: Option<ComponentExportIndex>,
    handle_http_index: Option<ComponentExportIndex>,
}

impl WasmActor {
    pub fn from_file<P: AsRef<Path>>(manifest_path: P) -> Result<Self> {
        let engine = Engine::default();

        // Read and parse manifest
        let manifest = std::fs::read_to_string(&manifest_path)
            .map_err(|e| WasmError::ManifestError(e.to_string()))?;
        let manifest: toml::Value =
            toml::from_str(&manifest).map_err(|e| WasmError::ManifestError(e.to_string()))?;

        // Get WASM file path
        let wasm_path = manifest["component_path"]
            .as_str()
            .ok_or_else(|| WasmError::ManifestError("Missing component_path".into()))?;

        // Read interfaces
        let implements = manifest["interfaces"]["implements"]
            .as_array()
            .ok_or_else(|| WasmError::ManifestError("Missing interfaces.implements".into()))?;

        // Check for required interfaces
        let has_actor = implements.iter().any(|i| {
            i.as_str()
                .map(|s| s == "ntwk:simple-actor/actor")
                .unwrap_or(false)
        });
        if !has_actor {
            return Err(WasmError::ManifestError(
                "Component must implement ntwk:simple-actor/actor".into(),
            )
            .into());
        }

        // Check for HTTP interface
        let has_http = implements.iter().any(|i| {
            i.as_str()
                .map(|s| s == "ntwk:simple-http-actor/http-actor")
                .unwrap_or(false)
        });

        // Load and instantiate component
        let wasm_bytes = std::fs::read(wasm_path)
            .map_err(|e| WasmError::ManifestError(format!("Failed to read WASM file: {}", e)))?;
        let component = Component::new(&engine, &wasm_bytes).map_err(|e| WasmError::WasmError {
            context: "component creation",
            message: e.to_string(),
        })?;

        // Set up linker with runtime functions
        let mut linker = Linker::new(&engine);
        let mut runtime =
            linker
                .instance("ntwk:simple-actor/runtime")
                .map_err(|e| WasmError::WasmError {
                    context: "runtime setup",
                    message: e.to_string(),
                })?;

        // Add log function
        runtime.func_wrap(
            "log",
            |_: wasmtime::StoreContextMut<'_, ()>, (msg,): (String,)| {
                println!("[WASM] {}", msg);
                Ok(())
            },
        )?;

        // Add send function
        runtime.func_wrap(
            "send",
            |_: wasmtime::StoreContextMut<'_, ()>, (actor_id, msg): (String, Vec<u8>)| {
                println!("Message send requested to {}", actor_id);
                // TODO: Implement actual message sending
                Ok(())
            },
        )?;

        // Get export indices for required functions
        let (_, actor_instance) = component
            .export_index(None, "ntwk:simple-actor/actor")
            .expect("Failed to get actor instance");

        let (_, init_index) = component
            .export_index(Some(&actor_instance), "init")
            .expect("Failed to get init index");

        let (_, handle_index) = component
            .export_index(Some(&actor_instance), "handle")
            .expect("Failed to get handle index");

        let (_, state_contract_index) = component
            .export_index(Some(&actor_instance), "state-contract")
            .expect("Failed to get state-contract index");

        let (_, message_contract_index) = component
            .export_index(Some(&actor_instance), "message-contract")
            .expect("Failed to get message-contract index");

        // Get HTTP-specific exports if available
        let (http_contract_index, handle_http_index) = if has_http {
            let (_, http_instance) = component
                .export_index(None, "ntwk:simple-http-actor/http-actor")
                .expect("Failed to get http-actor instance");

            let (_, http_contract) = component
                .export_index(Some(&http_instance), "http-contract")
                .expect("Failed to get http-contract index");

            let (_, handle_http) = component
                .export_index(Some(&http_instance), "handle-http")
                .expect("Failed to get handle-http index");

            (Some(http_contract), Some(handle_http))
        } else {
            (None, None)
        };

        Ok(WasmActor {
            engine,
            component,
            linker,
            init_index,
            handle_index,
            state_contract_index,
            message_contract_index,
            http_contract_index,
            handle_http_index,
        })
    }

    fn call_init(&self, store: &mut Store<()>, instance: &Instance) -> Result<Vec<u8>> {
        let init_func = instance
            .get_func(&mut *store, self.init_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "init function",
                message: "Function not found".into(),
            })?;

        let typed = init_func
            .typed::<(), (Vec<u8>,)>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "init function type",
                message: e.to_string(),
            })?;

        let (result,) = typed
            .call(&mut *store, ())
            .map_err(|e| WasmError::WasmError {
                context: "init function call",
                message: e.to_string(),
            })?;

        Ok(result)
    }

    fn call_handle(
        &self,
        store: &mut Store<()>,
        instance: &Instance,
        msg: Vec<u8>,
        state: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let handle_func = instance
            .get_func(&mut *store, self.handle_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "handle function",
                message: "Function not found".into(),
            })?;

        let typed = handle_func
            .typed::<(Vec<u8>, Vec<u8>), (Vec<u8>,)>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "handle function type",
                message: e.to_string(),
            })?;

        let (result,) =
            typed
                .call(&mut *store, (msg, state))
                .map_err(|e| WasmError::WasmError {
                    context: "handle function call",
                    message: e.to_string(),
                })?;

        Ok(result)
    }

    fn verify_state_contract(
        &self,
        store: &mut Store<()>,
        instance: &Instance,
        state: Vec<u8>,
    ) -> Result<bool> {
        let func = instance
            .get_func(&mut *store, self.state_contract_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "state-contract function",
                message: "Function not found".into(),
            })?;

        let typed = func
            .typed::<(Vec<u8>,), (bool,)>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "state-contract function type",
                message: e.to_string(),
            })?;

        let (result,) = typed
            .call(store, (state,))
            .map_err(|e| WasmError::WasmError {
                context: "state-contract function call",
                message: e.to_string(),
            })?;

        Ok(result)
    }
}

impl Actor for WasmActor {
    fn init(&self) -> Result<Value> {
        let mut store = Store::new(&self.engine, ());
        let instance = self.linker.instantiate(&mut store, &self.component)?;

        let result = self.call_init(&mut store, &instance)?;
        let state: Value = serde_json::from_slice(&result)?;

        Ok(state)
    }

    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)> {
        let mut store = Store::new(&self.engine, ());
        let instance = self.linker.instantiate(&mut store, &self.component)?;

        let state_bytes = serde_json::to_vec(state)?;

        match input {
            ActorInput::Message(msg) => {
                let msg_bytes = serde_json::to_vec(&msg)?;
                let result = self.call_handle(&mut store, &instance, msg_bytes, state_bytes)?;
                let new_state: Value = serde_json::from_slice(&result)?;
                Ok((ActorOutput::Message(msg), new_state))
            }
            ActorInput::HttpRequest {
                method,
                uri,
                headers,
                body,
            } => {
                if self.handle_http_index.is_none() {
                    return Err(anyhow::anyhow!("Actor does not support HTTP"));
                }

                let request = serde_json::json!({
                    "method": method,
                    "uri": uri,
                    "headers": { "fields": headers },
                    "body": body,
                });

                let request_bytes = serde_json::to_vec(&request)?;
                let result = self.call_handle(&mut store, &instance, request_bytes, state_bytes)?;

                let response: Value = serde_json::from_slice(&result)?;
                let new_state = response["state"].clone();
                let http_response = response["response"].clone();

                let status = http_response["status"].as_u64().unwrap_or(500) as u16;
                let headers = http_response["headers"]["fields"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| {
                                let pair = v.as_array()?;
                                Some((pair[0].as_str()?.to_string(), pair[1].as_str()?.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let body = http_response["body"]
                    .as_array()
                    .map(|arr| arr.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect());

                Ok((
                    ActorOutput::HttpResponse {
                        status,
                        headers,
                        body,
                    },
                    new_state,
                ))
            }
        }
    }

    fn verify_state(&self, state: &Value) -> bool {
        let mut store = Store::new(&self.engine, ());
        let instance = match self.linker.instantiate(&mut store, &self.component) {
            Ok(instance) => instance,
            Err(_) => return false,
        };

        let state_bytes = match serde_json::to_vec(state) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        self.verify_state_contract(&mut store, &instance, state_bytes)
            .unwrap_or(false)
    }
}
</file>
<file main.rs>
use anyhow::Result;
use runtime_v2::{ActorRuntime, WasmActor};
use std::path::PathBuf;
use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the actor manifest file
    #[arg(short, long)]
    manifest: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Load the WASM actor from the manifest
    println!("Loading actor from manifest: {:?}", args.manifest);
    let actor = WasmActor::from_file(args.manifest)?;

    // Create and initialize the runtime
    println!("Creating actor runtime...");
    let mut runtime = ActorRuntime::new(actor)?;
    
    println!("Initializing actor...");
    runtime.init().await?;

    println!("Actor initialized successfully!");
    println!("Current chain head: {:?}", runtime.get_chain().get_head());

    // TODO: Set up HTTP server or message handler based on manifest configuration
    
    // For now, just keep the program running
    println!("Actor is running. Press Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;
    
    println!("Shutting down...");
    Ok(())
}
</file>
<file Cargo.toml>
[package]
name = "runtime_v2"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0"
axum = "0.7"
futures = "0.3"
hyper = { version = "0.14", features = ["full"] }
md5 = "0.7.0"
reqwest = { version = "0.11", features = ["json"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["full"] }
thiserror = "1.0"
toml = "0.8"
uuid = { version = "1.0", features = ["v4"] }
wasmtime = { version = "27.0.0", features = ["component-model"] }
clap = { version = "4.4", features = ["derive"] }
axum-macros = "0.4.2"
</file>
</project>
