use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

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
pub trait Actor: Send {
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
    pub fn new(actor: Box<dyn Actor>, mailbox_rx: mpsc::Receiver<ActorMessage>) -> Result<Self> {
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

pub trait HostHandler: Send + Sync {
    fn name(&self) -> &str;
    fn new(config: Value) -> Self
    where
        Self: Sized;
    fn start(
        &self,
        mailbox_tx: mpsc::Sender<ActorMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn stop(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
}

#[derive(Debug, Clone)]
pub enum HostHandlerType {
    Http(Value),
    // Future handler types go here
}

// Manifest configuration
#[derive(Debug, Clone)]
pub struct ActorConfig {
    pub name: String,
    pub component_path: String,
    pub handlers: Vec<HostHandlerType>,
}

pub struct ActorRuntime {
    config: ActorConfig,
    handlers: Vec<Box<dyn HostHandler>>,
    process_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ActorRuntime {
    pub async fn from_file(manifest_path: PathBuf) -> Result<Self> {
        // Load actor from manifest
        let actor = Box::new(WasmActor::from_file(manifest_path)?);

        // Load config from manifest
        let config = ActorConfig {
            name: "Example Actor".to_string(),
            component_path: "example.wasm".to_string(),
            handlers: vec![HostHandlerType::Http(serde_json::json!({ "port": 8080 }))],
            //interfaces: vec![HostInterfaceType::Http(HttpConfig { port: 8080 })],
        };

        let (tx, rx) = mpsc::channel(32); // Buffer size of 32 messages

        // Create and spawn actor process
        let mut actor_process = ActorProcess::new(actor, rx)?;
        let process_handle = tokio::spawn(async move {
            if let Err(e) = actor_process.run().await {
                eprintln!("Actor process error: {}", e);
            }
        });

        // Create channel for actor process

        // Initialize host interfaces based on config
        let mut handlers: Vec<Box<dyn HostHandler>> = Vec::new();

        for handler_type in &config.handlers {
            match handler_type {
                HostHandlerType::Http(config) => {
                    let http_host = http::HttpHandler::new(config.clone());
                    handlers.push(Box::new(http_host));
                } // Add more interface types here as they're implemented
            }
        }

        // Start all host interfaces
        for handler in handlers.iter_mut() {
            handler.start(tx.clone()).await?;
        }

        Ok(Self {
            config,
            handlers,
            process_handle: Some(process_handle),
        })
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        // Stop all interfaces
        for handler in self.handlers.iter_mut() {
            handler.stop().await?;
        }

        // Cancel actor process
        if let Some(handle) = self.process_handle.take() {
            handle.abort();
        }

        Ok(())
    }
}
