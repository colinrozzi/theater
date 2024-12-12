use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use tokio::sync::{mpsc, oneshot};

pub mod capabilities;
pub mod chain;
pub mod config;
pub mod http;
pub mod http_server;
pub mod logging;
mod store;
mod wasm;

use tracing_subscriber::{EnvFilter, FmtSubscriber};

pub use config::{HandlerConfig, HttpHandlerConfig, HttpServerHandlerConfig, ManifestConfig};
pub use store::Store;
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

pub struct ActorRuntime {
    pub config: ManifestConfig,
    process_handle: Option<tokio::task::JoinHandle<()>>,
    handler_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl ActorRuntime {
    pub async fn from_file(manifest_path: PathBuf) -> Result<Self> {
        // Load manifest config
        let config = ManifestConfig::from_file(&manifest_path)?;

        // Initialize logging
        let _ = FmtSubscriber::builder()
            .with_env_filter(EnvFilter::new(
                config.logging.level.clone()
            ))
            .with_target(false)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .with_thread_names(true)
            .with_writer(std::io::stdout)
            .compact()
            .init();

        // Create store with HTTP handlers
        let (tx, rx) = mpsc::channel(32);
        let store = {
            let mut http_port = None;
            let mut http_server_port = None;

            // Find both handler ports
            for handler_config in &config.handlers {
                match handler_config {
                    HandlerConfig::Http(config) => http_port = Some(config.port),
                    HandlerConfig::HttpServer(config) => http_server_port = Some(config.port),
                }
            }

            // Initialize store based on which handlers we found
            match (http_port, http_server_port) {
                (Some(hp), Some(hsp)) => Store::with_both_http(hp, hsp, tx.clone()),
                (Some(p), None) => Store::with_http(p, tx.clone()),
                _ => Store::new(),
            }
        };

        // Create the WASM actor with the store
        let actor = Box::new(wasm::WasmActor::new(manifest_path, store)?);

        // Create and spawn actor process
        let mut actor_process = ActorProcess::new(actor, rx)?;
        let process_handle = tokio::spawn(async move {
            if let Err(e) = actor_process.run().await {
                eprintln!("Actor process error: {}", e);
            }
        });

        println!("Parsed config: {:#?}", config);
        let mut handler_tasks = Vec::new();
        println!("Initializing handlers...");
        println!("{:?}", config.handlers);
        for handler_config in &config.handlers {
            let tx = tx.clone();
            let handler_config = handler_config.clone();
            let task = tokio::spawn(async move {
                let handler: Box<dyn HostHandler> = match handler_config {
                    HandlerConfig::Http(http_config) => {
                        println!("[RUNTIME] Creating Http handler...");
                        Box::new(http::HttpHandler::new(http_config.port))
                    }
                    HandlerConfig::HttpServer(http_config) => {
                        println!("[RUNTIME] Creating Http-server handler...");
                        Box::new(http_server::HttpServerHandler::new(http_config.port))
                    }
                };

                let handler_name = handler.name().to_string();
                println!("[RUNTIME] Starting handler: {}", handler_name);

                let start_future = handler.start(tx.clone());
                match start_future.await {
                    Ok(_) => {
                        println!("[RUNTIME] Handler {} started successfully", handler_name);
                    }
                    Err(e) => {
                        eprintln!("[RUNTIME] Handler {} failed to start: {}", handler_name, e);
                    }
                }
            });

            handler_tasks.push(task);
        }

        println!(
            "[RUNTIME] All {} handlers started successfully",
            handler_tasks.len()
        );

        Ok(Self {
            config,
            process_handle: Some(process_handle),
            handler_tasks,
        })
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        // Stop all handlers
        for task in self.handler_tasks.drain(..) {
            task.abort();
        }

        // Cancel actor process
        if let Some(handle) = self.process_handle.take() {
            handle.abort();
        }

        Ok(())
    }
}
