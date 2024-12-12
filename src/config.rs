use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub name: String,
    pub component_path: PathBuf,
    #[serde(default)]
    pub interface: InterfacesConfig,
    #[serde(default)]
    pub handlers: Vec<HandlerConfig>,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub chain_events: bool,
    pub level: String,
    pub output: LogOutput,
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
            output: LogOutput::Stdout,
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
    Http(HttpHandlerConfig),
    #[serde(rename = "Http-server")]
    HttpServer(HttpServerHandlerConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHandlerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServerHandlerConfig {
    pub port: u16,
}

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
}
