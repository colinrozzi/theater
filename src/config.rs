use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    pub name: String,
    pub component_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManifestSource {
    Path(PathBuf),
    Content(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InitialStateSource {
    Path(PathBuf),
    Json(String),
    Remote(String), // For future use with URLs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub name: String,
    pub component_path: PathBuf,
    #[serde(default)]
    pub init_state: Option<InitialStateSource>,
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

impl ManifestConfig {
    pub fn from_string(content: &str) -> anyhow::Result<Self> {
        let mut config: ManifestConfig = toml::from_str(content)?;

        // Convert legacy init_state format if needed
        if let Some(init_state) = &config.init_state {
            if let InitialStateSource::Path(path) = init_state {
                if !path.exists() && !path.is_absolute() {
                    // Try to resolve the path relative to the current directory
                    let current_dir = std::env::current_dir()?;
                    let full_path = current_dir.join(path);
                    if full_path.exists() {
                        config.init_state = Some(InitialStateSource::Path(full_path));
                    }
                }
            }
        }

        Ok(config)
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(&path)?;
        let mut config = Self::from_string(&content)?;

        // Make component_path relative to manifest path if it's not absolute
        if !config.component_path.is_absolute() {
            if let Some(parent) = path.as_ref().parent() {
                config.component_path = parent.join(&config.component_path);
            }
        }

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
            Some(InitialStateSource::Path(path)) => {
                let data = std::fs::read(path)?;
                Ok(Some(data))
            }
            Some(InitialStateSource::Json(json_str)) => {
                // Validate the JSON string is proper JSON
                serde_json::from_str::<serde_json::Value>(json_str)?;
                Ok(Some(json_str.as_bytes().to_vec()))
            }
            Some(InitialStateSource::Remote(url)) => {
                // Placeholder for future implementation
                Err(anyhow::anyhow!("Remote state sources not yet implemented"))
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
