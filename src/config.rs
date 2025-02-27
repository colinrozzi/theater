use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    pub name: String,
    pub component_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub name: String,
    pub component_path: PathBuf,
    #[serde(default)]
    pub init_state: Option<PathBuf>,
    #[serde(default)]
    pub interface: InterfacesConfig,
    #[serde(default)]
    pub handlers: Vec<HandlerConfig>,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub event_server: Option<EventServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventServerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub chain_events: bool,
    pub level: String,
    pub output: LogOutput,
    pub log_dir: Option<PathBuf>,
    pub file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogOutput {
    Stdout,
    File,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            chain_events: true,
            level: "info".to_string(),
            output: LogOutput::File,
            log_dir: Some(PathBuf::from("logs")),
            file_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InterfacesConfig {
    #[serde(default)]
    pub implements: String,
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum HandlerConfig {
    #[serde(rename = "http-server")]
    HttpServer(HttpServerHandlerConfig),
    #[serde(rename = "message-server")]
    MessageServer(MessageServerConfig),
    #[serde(rename = "filesystem")]
    FileSystem(FileSystemHandlerConfig),
    #[serde(rename = "http-client")]
    HttpClient(HttpClientHandlerConfig),
    #[serde(rename = "runtime")]
    Runtime(RuntimeHostConfig),
    #[serde(rename = "websocket-server")]
    WebSocketServer(WebSocketServerHandlerConfig),
    #[serde(rename = "supervisor")]
    Supervisor(SupervisorHostConfig),
    #[serde(rename = "store")]
    Store(StoreHandlerConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorHostConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeHostConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServerHandlerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketServerHandlerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageServerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemHandlerConfig {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpClientHandlerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreHandlerConfig {}

impl ManifestConfig {
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ManifestConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn implements_interface(&self, interface_name: &str) -> bool {
        self.interface.implements == interface_name
    }

    pub fn interface(&self) -> &str {
        &self.interface.implements
    }

    pub fn load_init_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
        match &self.init_state {
            Some(path) => {
                let data = std::fs::read(path)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    pub fn message_server_port(&self) -> Option<u16> {
        for handler in &self.handlers {
            if let HandlerConfig::MessageServer(config) = handler {
                return Some(config.port);
            }
        }
        None
    }

    pub fn websocket_server_port(&self) -> Option<u16> {
        for handler in &self.handlers {
            if let HandlerConfig::WebSocketServer(config) = handler {
                return Some(config.port);
            }
        }
        None
    }

    pub fn http_server_port(&self) -> Option<u16> {
        for handler in &self.handlers {
            if let HandlerConfig::HttpServer(config) = handler {
                return Some(config.port);
            }
        }
        None
    }
}
