use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    pub name: String,
    pub component_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComponentSource {
    Path(PathBuf),
    Registry(String), // Registry URI
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManifestSource {
    Path(PathBuf),
    Content(String),
    Registry(String), // Registry URI
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InitialStateSource {
    Path(PathBuf),
    Json(String),
    Remote(String), // For future use with URLs
    Registry(String), // Registry URI
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub name: String,
    #[serde(alias = "component_path")]
    pub component_source: ComponentSource,
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

        // Make component_path relative to manifest path if it's path-based and not absolute
        if let ComponentSource::Path(component_path) = &config.component_source {
            if !component_path.is_absolute() {
                if let Some(parent) = path.as_ref().parent() {
                    config.component_source = ComponentSource::Path(parent.join(component_path));
                }
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
            Some(InitialStateSource::Remote(_url)) => {
                // Placeholder for future implementation
                Err(anyhow::anyhow!("Remote state sources not yet implemented"))
            }
            Some(InitialStateSource::Registry(_uri)) => {
                // This will be handled by the registry_manager before this method is called
                // The URI will be resolved to a temporary file and the path updated
                Err(anyhow::anyhow!("Registry state sources must be resolved before loading"))
            }
            None => Ok(None),
        }
    }

    pub fn resolve_resources(&mut self, registry_manager: &crate::registry::RegistryManager) -> anyhow::Result<()> {
        // Resolve component source if it's a registry URI
        if let ComponentSource::Registry(uri) = &self.component_source {
            let resource = registry_manager.resolve(uri)?;
            
            // Create a temporary file for the component
            let temp_dir = std::env::temp_dir().join("theater_registry");
            std::fs::create_dir_all(&temp_dir)?;
            
            let file_extension = if resource.content_type == "application/wasm" {
                ".wasm"
            } else {
                ".bin"
            };
            
            let temp_path = temp_dir.join(format!("{}-component{}", self.name, file_extension));
            std::fs::write(&temp_path, &resource.content)?;
            
            // Update the component source
            self.component_source = ComponentSource::Path(temp_path);
        }
        
        // Resolve init_state if it's a registry URI
        if let Some(InitialStateSource::Registry(uri)) = &self.init_state {
            let resource = registry_manager.resolve(uri)?;
            
            // Create a temporary file for the state
            let temp_dir = std::env::temp_dir().join("theater_registry");
            std::fs::create_dir_all(&temp_dir)?;
            
            let file_extension = if resource.content_type == "application/json" {
                ".json"
            } else {
                ".state"
            };
            
            let temp_path = temp_dir.join(format!("{}-state{}", self.name, file_extension));
            std::fs::write(&temp_path, &resource.content)?;
            
            // Update the init_state
            self.init_state = Some(InitialStateSource::Path(temp_path));
        }
        
        Ok(())
    }
    
    pub fn get_component_path(&self) -> anyhow::Result<PathBuf> {
        match &self.component_source {
            ComponentSource::Path(path) => Ok(path.clone()),
            ComponentSource::Registry(uri) => {
                Err(anyhow::anyhow!("Registry component source must be resolved before getting path: {}", uri))
            }
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
