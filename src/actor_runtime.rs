use crate::actor::Actor;
use crate::actor_process::ActorProcess;
use crate::config::{HandlerConfig, ManifestConfig};
use crate::host_handler::HostHandler;
use crate::http::HttpHandler;
use crate::http_server::HttpServerHandler;
use crate::store::Store;
use crate::Result;
use crate::WasmActor;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

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
        let actor = Box::new(WasmActor::new(&config, store)?);

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
                        Box::new(HttpHandler::new(http_config.port))
                    }
                    HandlerConfig::HttpServer(http_config) => {
                        Box::new(HttpServerHandler::new(http_config.port))
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
