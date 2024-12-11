use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub name: String,
    pub component_path: PathBuf,
    #[serde(default)]
    pub interfaces: InterfacesConfig,
    #[serde(default)]
    pub handlers: Vec<HandlerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InterfacesConfig {
    #[serde(default)]
    pub implements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum HandlerConfig {
    Http(HttpHandlerConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHandlerConfig {
    pub port: u16,
}

impl ManifestConfig {
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ManifestConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn implements_interface(&self, interface_name: &str) -> bool {
        self.interfaces.implements.iter().any(|i| i == interface_name)
    }
}