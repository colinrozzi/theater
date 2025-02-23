use crate::actor_runtime::WrappedActor;
use crate::host::filesystem::FileSystemHost;
use crate::host::http_client::HttpClientHost;
use crate::host::http_server::HttpServerHost;
use crate::host::message_server::MessageServerHost;
use crate::host::runtime::RuntimeHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::websocket_server::WebSocketServerHost;
use anyhow::Result;

pub enum Handler {
    MessageServer(MessageServerHost),
    HttpServer(HttpServerHost),
    FileSystem(FileSystemHost),
    HttpClient(HttpClientHost),
    Runtime(RuntimeHost),
    WebSocketServer(WebSocketServerHost),
    Supervisor(SupervisorHost),
}

impl Handler {
    pub async fn start(&mut self) -> Result<()> {
        match self {
            Handler::MessageServer(h) => h.start().await,
            Handler::HttpServer(h) => h.start().await,
            Handler::FileSystem(h) => h.start().await,
            Handler::HttpClient(h) => h.start().await,
            Handler::Runtime(h) => h.start().await,
            Handler::WebSocketServer(h) => h.start().await,
            Handler::Supervisor(h) => h.start().await,
        }
    }

    pub async fn setup_host_functions(&self, wrapped_actor: WrappedActor) -> Result<()> {
        match self {
            Handler::MessageServer(h) => Ok(h
                .setup_host_functions(wrapped_actor)
                .await
                .expect("Error setting up message server host functions")),
            Handler::HttpServer(h) => Ok(h
                .setup_host_functions(wrapped_actor)
                .await
                .expect("Error setting up http server host functions")),
            Handler::FileSystem(h) => Ok(h
                .setup_host_functions(wrapped_actor)
                .await
                .expect("Error setting up filesystem host functions")),
            Handler::HttpClient(h) => Ok(h
                .setup_host_functions(wrapped_actor)
                .await
                .expect("Error setting up http client host functions")),
            Handler::Runtime(h) => Ok(h
                .setup_host_functions(wrapped_actor)
                .await
                .expect("Error setting up runtime host functions")),
            Handler::WebSocketServer(h) => Ok(h
                .setup_host_functions(wrapped_actor)
                .await
                .expect("Error setting up websocket server host functions")),
            Handler::Supervisor(h) => Ok(h
                .setup_host_functions(wrapped_actor)
                .await
                .expect("Error setting up supervisor host functions")),
        }
    }

    pub async fn add_exports(&self, wrapped_actor: WrappedActor) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => Ok(handler
                .add_exports(wrapped_actor)
                .await
                .expect("Error adding exports to message server")),
            Handler::HttpServer(handler) => Ok(handler
                .add_exports(wrapped_actor)
                .await
                .expect("Error adding exports to http server")),
            Handler::FileSystem(handler) => Ok(handler
                .add_exports(wrapped_actor)
                .await
                .expect("Error adding exports to filesystem")),
            Handler::HttpClient(handler) => Ok(handler
                .add_exports(wrapped_actor)
                .await
                .expect("Error adding exports to http client")),
            Handler::Runtime(handler) => Ok(handler
                .add_exports(wrapped_actor)
                .await
                .expect("Error adding exports to runtime")),
            Handler::WebSocketServer(handler) => Ok(handler
                .add_exports(wrapped_actor)
                .await
                .expect("Error adding exports to websocket server")),
            Handler::Supervisor(handler) => Ok(handler
                .add_exports(wrapped_actor)
                .await
                .expect("Error adding exports to supervisor")),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Handler::MessageServer(_) => "message-server",
            Handler::HttpServer(_) => "http-server",
            Handler::FileSystem(_) => "filesystem",
            Handler::HttpClient(_) => "http-client",
            Handler::Runtime(_) => "runtime",
            Handler::WebSocketServer(_) => "websocket-server",
            Handler::Supervisor(_) => "supervisor",
        }
    }
}
