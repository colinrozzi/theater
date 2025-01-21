use crate::actor_handle::ActorHandle;
use crate::config::HandlerConfig;
use crate::host::filesystem::FileSystemHost;
use crate::host::http_client::HttpClientHost;
use crate::host::http_server::HttpServerHost;
use crate::host::message_server::MessageServerHost;
use crate::host::runtime::RuntimeHost;
use crate::Result;

pub enum Handler {
    MessageServer(MessageServerHost),
    HttpServer(HttpServerHost),
    FileSystem(FileSystemHost),
    HttpClient(HttpClientHost),
    Runtime(RuntimeHost),
}

impl Handler {
    pub fn new(handler_config: HandlerConfig, actor_handle: ActorHandle) -> Self {
        match handler_config {
            HandlerConfig::MessageServer(config) => {
                Handler::MessageServer(MessageServerHost::new(config, actor_handle))
            }
            HandlerConfig::HttpServer(config) => {
                Handler::HttpServer(HttpServerHost::new(config, actor_handle))
            }
            HandlerConfig::FileSystem(config) => {
                Handler::FileSystem(FileSystemHost::new(config, actor_handle))
            }
            HandlerConfig::HttpClient(config) => {
                Handler::HttpClient(HttpClientHost::new(config, actor_handle))
            }
            HandlerConfig::Runtime(config) => {
                Handler::Runtime(RuntimeHost::new(config, actor_handle))
            }
        }
    }

    pub async fn start(&self) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler.start().await,
            Handler::HttpServer(handler) => handler.start().await,
            Handler::FileSystem(handler) => handler.start().await,
            Handler::HttpClient(handler) => handler.start().await,
            Handler::Runtime(handler) => handler.start().await,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Handler::MessageServer(_) => "message-server".to_string(),
            Handler::HttpServer(_) => "http-server".to_string(),
            Handler::FileSystem(_) => "filesystem".to_string(),
            Handler::HttpClient(_) => "http-client".to_string(),
            Handler::Runtime(_) => "runtime".to_string(),
        }
    }

    pub async fn setup_host_function(&self) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler
                .setup_host_functions()
                .await
                .expect("could not setup host functions for message server"),
            Handler::HttpServer(handler) => handler
                .setup_host_functions()
                .await
                .expect("could not setup host functions for http server"),
            Handler::FileSystem(handler) => handler
                .setup_host_functions()
                .await
                .expect("could not setup host functions for filesystem"),
            Handler::HttpClient(handler) => handler
                .setup_host_functions()
                .await
                .expect("could not setup host functions for http client"),
            Handler::Runtime(handler) => handler
                .setup_host_functions()
                .await
                .expect("could not setup host functions for runtime"),
        }
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => Ok(handler
                .add_exports()
                .await
                .expect("could not add exports for message server")),
            Handler::HttpServer(handler) => Ok(handler
                .add_exports()
                .await
                .expect("could not add exports for http server")),
            Handler::FileSystem(handler) => Ok(handler
                .add_exports()
                .await
                .expect("could not add exports for filesystem")),
            Handler::HttpClient(handler) => Ok(handler
                .add_exports()
                .await
                .expect("could not add exports for http client")),
            Handler::Runtime(handler) => Ok(handler
                .add_exports()
                .await
                .expect("could not add exports for runtime")),
        }
    }
}
