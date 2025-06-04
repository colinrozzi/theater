use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tracing::debug;

/// Theater CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub output: OutputConfig,
    pub logging: LoggingConfig,
    pub templates: TemplatesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub default_address: SocketAddr,
    pub timeout: Duration,
    pub retry_attempts: u32,
    pub retry_delay: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub default_format: String,
    pub colors: bool,
    pub timestamps: bool,
    pub max_width: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file: Option<PathBuf>,
    pub structured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplatesConfig {
    pub directories: Vec<PathBuf>,
    pub auto_update: bool,
    pub cache_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                default_address: "127.0.0.1:9000".parse().unwrap(),
                timeout: Duration::from_secs(30),
                retry_attempts: 3,
                retry_delay: Duration::from_millis(1000),
            },
            output: OutputConfig {
                default_format: "compact".to_string(),
                colors: true,
                timestamps: true,
                max_width: None,
            },
            logging: LoggingConfig {
                level: "warn".to_string(),
                file: None,
                structured: false,
            },
            templates: TemplatesConfig {
                directories: vec![],
                auto_update: true,
                cache_dir: None,
            },
        }
    }
}

impl Config {
    /// Load configuration from the standard locations
    pub fn load() -> Result<Self> {
        let mut config = Self::default();

        // Try to load from user config directory
        if let Ok(config_dir) = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".config"))
                    .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
            })
        {
            let config_file = config_dir.join("theater").join("config.toml");
            if config_file.exists() {
                debug!("Loading config from {}", config_file.display());
                config = Self::load_from_file(&config_file).with_context(|| {
                    format!("Failed to load config from {}", config_file.display())
                })?;
            }
        }

        // Override with environment variables
        config.apply_env_overrides();

        Ok(config)
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Self = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Apply environment variable overrides
    fn apply_env_overrides(&mut self) {
        if let Ok(addr) = std::env::var("THEATER_SERVER_ADDRESS") {
            if let Ok(parsed_addr) = addr.parse() {
                self.server.default_address = parsed_addr;
            }
        }

        if let Ok(level) = std::env::var("THEATER_LOG_LEVEL") {
            self.logging.level = level;
        }

        if let Ok(colors) = std::env::var("THEATER_COLORS") {
            self.output.colors = colors.parse().unwrap_or(true);
        }
    }

    /// Get the config directory for this user
    pub fn config_dir() -> Result<PathBuf> {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".config"))
                    .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
            })
            .map(|dir| dir.join("theater"))
    }

    /// Get the cache directory for this user
    pub fn cache_dir() -> Result<PathBuf> {
        std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".cache"))
                    .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
            })
            .map(|dir| dir.join("theater"))
    }

    /// Get the state directory for this user
    pub fn state_dir() -> Result<PathBuf> {
        std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".local").join("state"))
                    .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
            })
            .map(|dir| dir.join("theater"))
    }

    /// Save configuration to the default location
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        std::fs::create_dir_all(&config_dir).with_context(|| {
            format!(
                "Failed to create config directory: {}",
                config_dir.display()
            )
        })?;

        let config_file = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        std::fs::write(&config_file, content)
            .with_context(|| format!("Failed to write config file: {}", config_file.display()))?;

        debug!("Saved config to {}", config_file.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(
            config.server.default_address,
            "127.0.0.1:9000".parse().unwrap()
        );
        assert_eq!(config.output.default_format, "compact");
        assert!(config.output.colors);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(
            config.server.default_address,
            deserialized.server.default_address
        );
        assert_eq!(
            config.output.default_format,
            deserialized.output.default_format
        );
    }

    #[test]
    fn test_config_load_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let config_content = r#"
[server]
default_address = "192.168.1.100:8080"
timeout = "60s"
retry_attempts = 5
retry_delay = "2s"

[output]
default_format = "json"
colors = false
timestamps = false

[logging]
level = "debug"
structured = true
"#;

        std::fs::write(&config_file, config_content).unwrap();
        let config = Config::load_from_file(&config_file).unwrap();

        assert_eq!(
            config.server.default_address,
            "192.168.1.100:8080".parse().unwrap()
        );
        assert_eq!(config.output.default_format, "json");
        assert!(!config.output.colors);
        assert_eq!(config.logging.level, "debug");
    }
}
