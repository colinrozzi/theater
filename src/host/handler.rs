use crate::actor_handle::ActorHandle;
use crate::config::HandlerConfig;
use crate::host::filesystem::FileSystemHost;
use crate::host::http_server::HttpServerHost;
use crate::host::message_server::MessageServerHost;
use crate::Result;

pub enum Handler {
    MessageServer(MessageServerHost),
    HttpServer(HttpServerHost),
    FileSystem(FileSystemHost),
}

impl Handler {
    pub fn new(handler_config: HandlerConfig, actor_handle: ActorHandle) -> Self {
        match handler_config {
            HandlerConfig::MessageServer(config) => {
                Handler::MessageServer(MessageServerHost::new(config.port, actor_handle))
            }
            HandlerConfig::HttpServer(config) => {
                Handler::HttpServer(HttpServerHost::new(config.port, actor_handle))
            }
            HandlerConfig::FileSystem(config) => {
                Handler::FileSystem(FileSystemHost::new(config.path, actor_handle))
            }
        }
    }

    pub async fn start(&self) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler.start().await,
            Handler::HttpServer(handler) => handler.start().await,
            Handler::FileSystem(handler) => handler.start().await,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Handler::MessageServer(_) => "message-server".to_string(),
            Handler::HttpServer(_) => "http-server".to_string(),
            Handler::FileSystem(_) => "filesystem".to_string(),
        }
    }

    pub async fn setup_host_function(&self) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler.setup_host_functions()?,
            Handler::HttpServer(handler) => handler.setup_host_functions()?,
            Handler::FileSystem(handler) => handler.setup_host_functions().await?,
        }
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => Ok(handler.add_exports().await?),
            Handler::HttpServer(handler) => Ok(handler.add_exports().await?),
            Handler::FileSystem(handler) => Ok(handler.add_exports().await?),
        }
    }
}
