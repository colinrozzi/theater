use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

pub mod capabilities;
pub mod chain;
pub mod chain_emitter;
pub mod config;
pub mod event_server;
pub mod http;
pub mod http_server;
pub mod logging;
mod store;
mod wasm;

use chain::{ChainEvent, HashChain};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

pub use config::{HandlerConfig, HttpHandlerConfig, HttpServerHandlerConfig, ManifestConfig};
pub use store::Store;
pub use wasm::{WasmActor, WasmError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorInput {
    Message(Value),
    HttpRequest {
        method: String,
        uri: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorOutput {
    Message(Value),
    HttpResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
}

#[derive(Debug)]
pub enum MessageMetadata {
    ActorSource {
        source_actor: String,
        source_chain_state: String,
    },
    HttpRequest {
        response_channel: oneshot::Sender<ActorOutput>,
    },
}

#[derive(Debug)]
pub struct ActorMessage {
    pub content: ActorInput,
    pub metadata: Option<MessageMetadata>,
}

pub trait Actor: Send {
    fn init(&self) -> Result<Value>;
    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)>;
    fn verify_state(&self, state: &Value) -> bool;
}

pub struct ActorProcess {
    mailbox_rx: mpsc::Receiver<ActorMessage>,
    chain: HashChain,
    actor: Box<dyn Actor>,
    name: String,
}

impl ActorProcess {
    pub fn new(
        name: &String,
        actor: Box<dyn Actor>,
        mailbox_rx: mpsc::Receiver<ActorMessage>,
    ) -> Result<Self> {
        let mut chain = HashChain::new();

        // Initialize with initial state
        let initial_state = actor.init()?;
        chain.add_event(ChainEvent::StateChange {
            old_state: Value::Null,
            new_state: initial_state,
            timestamp: Utc::now(),
        });

        Ok(Self {
            mailbox_rx,
            chain,
            actor,
            name: name.to_string(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Some(msg) = self.mailbox_rx.recv().await {
            // Record appropriate chain event based on message type
            match &msg.metadata {
                Some(MessageMetadata::ActorSource {
                    source_actor,
                    source_chain_state,
                }) => {
                    self.chain.add_event(ChainEvent::ActorMessage {
                        source_actor: source_actor.clone(),
                        source_chain_state: source_chain_state.clone(),
                        content: match &msg.content {
                            ActorInput::Message(v) => v.clone(),
                            _ => serde_json::to_value(&msg.content).unwrap_or_default(),
                        },
                        timestamp: Utc::now(),
                    });
                }
                _ => {
                    self.chain.add_event(ChainEvent::ExternalInput {
                        input: msg.content.clone(),
                        timestamp: Utc::now(),
                    });
                }
            }

            // Get current state from chain
            let current_state = self
                .chain
                .get_current_state()
                .ok_or_else(|| anyhow::anyhow!("No current state found in chain"))?;

            // Process input
            let (output, new_state) = self.actor.handle_input(msg.content, &current_state)?;

            // Record state change
            let state_hash = self.chain.add_event(ChainEvent::StateChange {
                old_state: current_state,
                new_state: new_state.clone(),
                timestamp: Utc::now(),
            });

            // Record output
            self.chain.add_event(ChainEvent::Output {
                output: output.clone(),
                chain_state: state_hash,
                timestamp: Utc::now(),
            });

            // Send response if metadata contains response channel
            if let Some(MessageMetadata::HttpRequest { response_channel }) = msg.metadata {
                let _ = response_channel.send(output);
            }
        }

        Ok(())
    }

    pub fn get_chain(&self) -> &HashChain {
        &self.chain
    }

    pub fn send_message(&mut self, target: &str, msg: Value) -> Result<()> {
        // First record the send event
        let current_chain_state = self
            .chain
            .get_head()
            .ok_or_else(|| anyhow::anyhow!("No chain head"))?
            .to_string();

        self.chain.add_event(ChainEvent::ActorMessage {
            source_actor: self.name.clone(),
            source_chain_state: current_chain_state.clone(),
            content: msg.clone(),
            timestamp: Utc::now(),
        });

        // TODO: Implement actual message sending with metadata
        Ok(())
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
            .with_env_filter(EnvFilter::new(config.logging.level.clone()))
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
        let actor = Box::new(wasm::WasmActor::new(&config, store)?);

        // Create and spawn actor process
        let mut actor_process = ActorProcess::new(&config.name, actor, rx)?;
        let process_handle = tokio::spawn(async move {
            if let Err(e) = actor_process.run().await {
                error!("Actor process failed: {}", e);
            }
        });

        let mut handler_tasks = Vec::new();
        for handler_config in &config.handlers {
            let tx = tx.clone();
            let handler_config = handler_config.clone();
            let task = tokio::spawn(async move {
                let handler: Box<dyn HostHandler> = match handler_config {
                    HandlerConfig::Http(http_config) => {
                        Box::new(http::HttpHandler::new(http_config.port))
                    }
                    HandlerConfig::HttpServer(http_config) => {
                        Box::new(http_server::HttpServerHandler::new(http_config.port))
                    }
                };

                let handler_name = handler.name().to_string();

                let start_future = handler.start(tx.clone());
                match start_future.await {
                    Ok(_) => {
                        info!("Handler {} started successfully", handler_name);
                    }
                    Err(e) => {
                        error!("Failed to start handler: {}", e);
                    }
                }
            });

            handler_tasks.push(task);
        }

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
