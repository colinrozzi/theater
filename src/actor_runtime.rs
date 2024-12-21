use crate::wasm::{Event, WasmActor};
use crate::actor_process::ActorMessage;
use crate::actor_process::ActorProcess;
use crate::chain::{ChainEntry, HashChain};
use crate::config::{HandlerConfig, ManifestConfig};
use crate::http_server::HttpServerHost;
use crate::message_server::MessageServerHost;
use crate::store::Store;
use crate::Result;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::{error, info};

pub struct RuntimeComponents {
    handlers: Vec<Handler>,
    actor_process: ActorProcess,
    chain_handler: ChainRequestHandler,
    actor_tx: mpsc::Sender<ActorMessage>,
}

pub struct ActorRuntime {
    chain_task: tokio::task::JoinHandle<()>,
    process_handle: Option<tokio::task::JoinHandle<()>>,
    handler_tasks: Vec<tokio::task::JoinHandle<()>>,
}

pub struct ChainRequest {
    pub request_type: ChainRequestType,
    pub response_tx: oneshot::Sender<ChainResponse>,
}

#[derive(Debug)]
pub enum ChainRequestType {
    GetHead,
    GetChainEntry(String),
    GetChain,
    AddEvent { event: Event },
}

pub enum ChainResponse {
    Head(Option<String>),
    ChainEntry(Option<ChainEntry>),
    FullChain(Vec<(String, ChainEntry)>),
}

#[derive(Clone, Debug)]
pub enum Handler {
    MessageServer(MessageServerHost),
    HttpServer(HttpServerHost),
}

impl Handler {
    pub async fn start(&self, actor_tx: mpsc::Sender<ActorMessage>) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler.start(actor_tx).await,
            Handler::HttpServer(handler) => handler.start(actor_tx).await,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Handler::MessageServer(_) => "message-server".to_string(),
            Handler::HttpServer(_) => "http-server".to_string(),
        }
    }
}

impl ActorRuntime {
    pub async fn from_file(manifest_path: PathBuf) -> Result<RuntimeComponents> {
        // Load manifest config
        let config = ManifestConfig::from_file(&manifest_path)?;
        let runtime = Self::new(&config)?;
        Ok(runtime)
    }

    pub fn new(config: &ManifestConfig) -> Result<RuntimeComponents> {
        Self::init_components(config)
    }

    fn init_components(config: &ManifestConfig) -> Result<RuntimeComponents> {
        let (chain_tx, chain_rx) = mpsc::channel(32);
        let (actor_tx, actor_rx) = mpsc::channel(32);

        let handlers = config
            .handlers
            .iter()
            .map(|handler_config| match handler_config {
                HandlerConfig::MessageServer(config) => {
                    Handler::MessageServer(MessageServerHost::new(config.port))
                }
                HandlerConfig::HttpServer(config) => {
                    Handler::HttpServer(HttpServerHost::new(config.port))
                }
            })
            .collect();

        let store = Store::new(chain_tx.clone());
        let actor = WasmActor::new(config, store)?;

        // Create and spawn actor process
        let actor_process = ActorProcess::new(&config.name, actor, actor_rx, chain_tx)?;

        let chain_handler = ChainRequestHandler::new(chain_rx);

        Ok(RuntimeComponents {
            handlers,
            actor_process,
            chain_handler,
            actor_tx,
        })
    }

    pub async fn start(mut components: RuntimeComponents) -> Result<Self> {
        let mut handler_tasks = Vec::new();

        // Start all handlers
        for handler in components.handlers {
            let handler_clone = handler.clone();
            let tx = components.actor_tx.clone();
            let task = tokio::spawn(async move {
                if let Err(e) = handler_clone.start(tx).await {
                    error!("Handler failed: {}", e);
                }
            });
            handler_tasks.push(task);
        }

        // Start the actor process
        let process_handle = Some(tokio::spawn(async move {
            if let Err(e) = components.actor_process.run().await {
                error!("Actor process failed: {}", e);
            }
        }));

        info!("Actor runtime started");

        // spawn the chain request handler
        let chain_task = tokio::spawn(async move {
            components.chain_handler.run().await;
        });

        Ok(Self {
            chain_task,
            process_handle,
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

        // Cancel chain task
        self.chain_task.abort();

        Ok(())
    }
}

struct ChainRequestHandler {
    chain: HashChain,
    chain_rx: mpsc::Receiver<ChainRequest>,
}

impl ChainRequestHandler {
    pub fn new(chain_rx: mpsc::Receiver<ChainRequest>) -> Self {
        let chain = HashChain::new();
        Self { chain, chain_rx }
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(req) = self.chain_rx.recv() => {
                    self.handle_chain_request(req).await;
                }
                else => {
                    info!("Chain request handler shutting down");
                    break;
                }
            }
        }
    }

    pub async fn handle_chain_request(&mut self, req: ChainRequest) {
        let response = match req.request_type {
            ChainRequestType::GetHead => {
                let head = self.chain.get_head().map(|h| h.to_string());
                ChainResponse::Head(head)
            }
            ChainRequestType::GetChainEntry(hash) => {
                let entry = self.chain.get_chain_entry(&hash);
                ChainResponse::ChainEntry(entry.cloned())
            }
            ChainRequestType::GetChain => {
                let full_chain = self.chain.get_full_chain();
                ChainResponse::FullChain(full_chain)
            }
            ChainRequestType::AddEvent { event } => {
                let hash = self.chain.add(event);
                ChainResponse::Head(Some(hash))
            }
        };

        let _ = req.response_tx.send(response);
    }
}