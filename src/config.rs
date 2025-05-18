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
    pub version: String,
    pub component_path: String,
    pub description: Option<String>,
    pub long_description: Option<String>,
    pub save_chain: Option<bool>,
    #[serde(default)]
    pub init_state: Option<String>,
    #[serde(default)]
    pub handlers: Vec<HandlerConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum HandlerConfig {
    #[serde(rename = "message-server")]
    MessageServer(MessageServerConfig),
    #[serde(rename = "filesystem")]
    FileSystem(FileSystemHandlerConfig),
    #[serde(rename = "http-client")]
    HttpClient(HttpClientHandlerConfig),
    #[serde(rename = "http-framework")]
    HttpFramework(HttpFrameworkHandlerConfig),
    #[serde(rename = "runtime")]
    Runtime(RuntimeHostConfig),
    #[serde(rename = "supervisor")]
    Supervisor(SupervisorHostConfig),
    #[serde(rename = "store")]
    Store(StoreHandlerConfig),
    #[serde(rename = "timing")]
    Timing(TimingHostConfig),
    #[serde(rename = "process")]
    Process(ProcessHostConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorHostConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeHostConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingHostConfig {
    #[serde(default = "default_max_sleep_duration")]
    pub max_sleep_duration: u64,
    #[serde(default = "default_min_sleep_duration")]
    pub min_sleep_duration: u64,
}

fn default_max_sleep_duration() -> u64 {
    // Default to 1 hour maximum sleep duration (in milliseconds)
    3600000
}

fn default_min_sleep_duration() -> u64 {
    // Default to 1 millisecond minimum sleep duration
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServerHandlerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketServerHandlerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageServerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileSystemHandlerConfig {
    pub path: Option<PathBuf>,
    pub new_dir: Option<bool>,
    pub allowed_commands: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpClientHandlerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoreHandlerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpFrameworkHandlerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessHostConfig {
    #[serde(default = "default_max_processes")]
    pub max_processes: usize,
    #[serde(default = "default_max_output_buffer")]
    pub max_output_buffer: usize,
    pub allowed_programs: Option<Vec<String>>,
    pub allowed_paths: Option<Vec<String>>,
}

fn default_max_processes() -> usize {
    // Default to 10 processes per actor
    10
}

fn default_max_output_buffer() -> usize {
    // Default to 1MB max output buffer per process
    1024 * 1024
}

impl ManifestConfig {
    /// Loads a manifest configuration from a TOML file.
    ///
    /// ## Purpose
    ///
    /// This method reads a manifest file from disk, parses it as TOML, and constructs
    /// a ManifestConfig instance. It's the primary way to load actor configurations
    /// from the filesystem.
    ///
    /// ## Parameters
    ///
    /// * `path` - Path to the TOML manifest file
    ///
    /// ## Returns
    ///
    /// * `Ok(ManifestConfig)` - The successfully parsed configuration
    /// * `Err(anyhow::Error)` - If the file cannot be read or contains invalid TOML
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::config::ManifestConfig;
    /// use std::path::Path;
    ///
    /// fn example() -> anyhow::Result<()> {
    ///     let config = ManifestConfig::from_file(Path::new("manifest.toml"))?;
    ///     println!("Loaded actor: {}", config.name);
    ///     Ok(())
    /// }
    /// ```
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ManifestConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Loads a manifest configuration from a TOML string.
    ///
    /// ## Purpose
    ///
    /// This method parses a string containing TOML data and constructs a ManifestConfig
    /// instance. This is useful when the manifest content is available in memory rather
    /// than in a file.
    ///
    /// ## Parameters
    ///
    /// * `content` - TOML string containing the manifest configuration
    ///
    /// ## Returns
    ///
    /// * `Ok(ManifestConfig)` - The successfully parsed configuration
    /// * `Err(anyhow::Error)` - If the string contains invalid TOML
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::config::ManifestConfig;
    ///
    /// fn example() -> anyhow::Result<()> {
    ///     let toml_content = r#"
    ///         name = "example-actor"
    ///         component_path = "./example.wasm"
    ///     "#;
    ///     
    ///     let config = ManifestConfig::from_str(toml_content)?;
    ///     println!("Loaded actor: {}", config.name);
    ///     Ok(())
    /// }
    /// ```
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let config: ManifestConfig = toml::from_str(content)?;
        Ok(config)
    }

    /// Loads a manifest configuration from a byte vector.
    ///
    /// ## Purpose
    ///
    /// This method converts a byte vector to a UTF-8 string, parses it as TOML,
    /// and constructs a ManifestConfig instance. This is useful when the manifest
    /// content is available as raw bytes, such as when loaded from a content store.
    ///
    /// ## Parameters
    ///
    /// * `content` - Byte vector containing UTF-8 encoded TOML data
    ///
    /// ## Returns
    ///
    /// * `Ok(ManifestConfig)` - The successfully parsed configuration
    /// * `Err(anyhow::Error)` - If the bytes cannot be converted to valid UTF-8 or contain invalid TOML
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::config::ManifestConfig;
    ///
    /// fn example() -> anyhow::Result<()> {
    ///     let bytes = vec![/* ... */];
    ///     let config = ManifestConfig::from_vec(bytes)?;
    ///     println!("Loaded actor: {}", config.name);
    ///     Ok(())
    /// }
    /// ```
    pub fn from_vec(content: Vec<u8>) -> anyhow::Result<Self> {
        let config: ManifestConfig = toml::from_str(&String::from_utf8(content)?)?;
        Ok(config)
    }

    /// Gets the name of the actor.
    ///
    /// ## Purpose
    ///
    /// This method provides access to the actor's name, which is its primary
    /// identifier in logs and diagnostics.
    ///
    /// ## Returns
    ///
    /// A string reference to the actor's name
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::config::ManifestConfig;
    /// # fn example(config: &ManifestConfig) {
    /// println!("Actor name: {}", config.name());
    /// # }
    /// ```
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Loads the initial state data for the actor.
    ///
    /// ## Purpose
    ///
    /// This method reads the initial state data from the file specified in the
    /// `init_state` field, if present. This data is used to initialize the actor
    /// when it starts.
    ///
    /// ## Returns
    ///
    /// * `Ok(Some(Vec<u8>))` - The initial state data if specified and successfully loaded
    /// * `Ok(None)` - If no initial state file is specified
    /// * `Err(anyhow::Error)` - If the initial state file cannot be read
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::config::ManifestConfig;
    /// # fn example(config: &ManifestConfig) -> anyhow::Result<()> {
    /// if let Some(state_data) = config.load_init_state()? {
    ///     println!("Loaded initial state: {} bytes", state_data.len());
    /// } else {
    ///     println!("No initial state specified");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_init_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
        match &self.init_state {
            Some(path) => {
                let data = std::fs::read(path)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Converts the manifest to a fixed byte representation.
    ///
    /// ## Purpose
    ///
    /// This method serializes the manifest to a standardized byte representation
    /// suitable for content-addressed storage. The goal is to ensure that identical
    /// manifests produce identical byte representations, enabling deduplication.
    ///
    /// ## Returns
    ///
    /// A byte vector containing the serialized manifest
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::config::ManifestConfig;
    /// # fn example(config: ManifestConfig) {
    /// let bytes = config.into_fixed_bytes();
    /// println!("Serialized manifest: {} bytes", bytes.len());
    /// # }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This is intended to store a manifest in the content store in such a way that there is only
    /// one representation per possible manifest. The current implementation uses TOML serialization,
    /// but this might be refined in the future to guarantee consistent representations.
    pub fn into_fixed_bytes(self) -> Vec<u8> {
        toml::to_string(&self).unwrap().into_bytes()
    }

    pub fn save_chain(&self) -> bool {
        self.save_chain.unwrap_or(false)
    }
}
