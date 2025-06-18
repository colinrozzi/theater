use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::debug;

use crate::utils::resolve_reference;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    pub name: String,
    pub component_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub name: String,
    pub version: String,
    pub component: String,
    pub description: Option<String>,
    pub long_description: Option<String>,
    pub save_chain: Option<bool>,
    pub permissions: Option<HandlerPermission>,
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
pub struct HandlerPermission {
    message_server: Option<MessageServerPermissions>,
    file_system: Option<FileSystemPermissions>,
    http_client: Option<HttpClientPermissions>,
    http_framework: Option<HttpFrameworkPermissions>,
    runtime: Option<RuntimePermissions>,
    supervisor: Option<SupervisorPermissions>,
    store: Option<StorePermissions>,
    timing: Option<TimingPermissions>,
    process: Option<ProcessPermissions>,
    environment: Option<EnvironmentPermissions>,
    random: Option<RandomPermissions>,
}

impl PartialOrd for HandlerPermission {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use std::cmp::Ordering;

        let comparisons = [
            self.message_server.partial_cmp(&other.message_server),
            self.file_system.partial_cmp(&other.file_system),
            self.http_client.partial_cmp(&other.http_client),
            self.http_framework.partial_cmp(&other.http_framework),
            self.runtime.partial_cmp(&other.runtime),
            self.supervisor.partial_cmp(&other.supervisor),
            self.store.partial_cmp(&other.store),
            self.timing.partial_cmp(&other.timing),
            self.process.partial_cmp(&other.process),
            self.environment.partial_cmp(&other.environment),
            self.random.partial_cmp(&other.random),
        ];

        let mut any_strict = false;

        for cmp in comparisons {
            match cmp {
                Some(Ordering::Greater) => any_strict = true,
                Some(Ordering::Equal) => continue,
                _ => return None, // Less or incomparable
            }
        }

        Some(if any_strict {
            Ordering::Greater
        } else {
            Ordering::Equal
        })
    }
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
    #[serde(rename = "environment")]
    Environment(EnvironmentHandlerConfig),
    #[serde(rename = "random")]
    Random(RandomHandlerConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorHostConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct SupervisorPermissions {
    pub max_children: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeHostConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimePermissions {}

impl PartialOrd for RuntimePermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
        Some(std::cmp::Ordering::Equal)
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct TimingPermissions {
    pub max_sleep_duration: u64,
    pub min_sleep_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServerHandlerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpServerPermissions {}

impl PartialOrd for HttpServerPermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
        Some(std::cmp::Ordering::Equal)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketServerHandlerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebSocketServerPermissions {}

impl PartialOrd for WebSocketServerPermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
        Some(std::cmp::Ordering::Equal)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageServerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageServerPermissions {}

impl PartialOrd for MessageServerPermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
        Some(std::cmp::Ordering::Equal)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileSystemHandlerConfig {
    pub path: Option<PathBuf>,
    pub new_dir: Option<bool>,
    pub allowed_commands: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileSystemPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub allowed_commands: Option<Vec<String>>,
    pub new_dir: Option<bool>,
    pub allowed_paths: Option<Vec<String>>,
}

impl PartialOrd for FileSystemPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // The parent must have >= permissions than the child.

        // Booleans: parent must be >= child
        if self.read < other.read || self.write < other.write || self.execute < other.execute {
            return None;
        }

        // new_dir: treat None as false
        if self.new_dir.unwrap_or(false) < other.new_dir.unwrap_or(false) {
            return None;
        }

        // allowed_commands: parent must be a superset
        if let Some(child_cmds) = &other.allowed_commands {
            if let Some(parent_cmds) = &self.allowed_commands {
                let parent_set: HashSet<_> = parent_cmds.iter().collect();
                let child_set: HashSet<_> = child_cmds.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // allowed_paths: same logic
        if let Some(child_paths) = &other.allowed_paths {
            if let Some(parent_paths) = &self.allowed_paths {
                let parent_set: HashSet<_> = parent_paths.iter().collect();
                let child_set: HashSet<_> = child_paths.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // If all checks passed, child is ≤ parent
        if self == other {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater) // "Parent > Child"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpClientHandlerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpClientPermissions {
    pub allowed_methods: Option<Vec<String>>,
    pub allowed_hosts: Option<Vec<String>>,
    pub max_redirects: Option<usize>,
    pub timeout: Option<u64>, // in milliseconds
}

impl PartialOrd for HttpClientPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // allowed_methods: child must be subset of parent
        if let Some(child_methods) = &other.allowed_methods {
            if let Some(parent_methods) = &self.allowed_methods {
                let parent_set: HashSet<_> = parent_methods.iter().collect();
                let child_set: HashSet<_> = child_methods.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // allowed_hosts: child must be subset of parent
        if let Some(child_hosts) = &other.allowed_hosts {
            if let Some(parent_hosts) = &self.allowed_hosts {
                let parent_set: HashSet<_> = parent_hosts.iter().collect();
                let child_set: HashSet<_> = child_hosts.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // max_redirects: child must be <= parent
        if let Some(child_max) = other.max_redirects {
            if let Some(parent_max) = self.max_redirects {
                if child_max > parent_max {
                    return None;
                }
            } else {
                return None;
            }
        }

        // timeout: child must be <= parent
        if let Some(child_timeout) = other.timeout {
            if let Some(parent_timeout) = self.timeout {
                if child_timeout > parent_timeout {
                    return None;
                }
            } else {
                return None;
            }
        }

        if self == other {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoreHandlerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct StorePermissions {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpFrameworkHandlerConfig {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpFrameworkPermissions {
    pub allowed_routes: Option<Vec<String>>,
    pub allowed_methods: Option<Vec<String>>,
    pub max_request_size: Option<usize>, // in bytes
}

impl PartialOrd for HttpFrameworkPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // allowed_routes: child must be subset of parent
        if let Some(child_routes) = &other.allowed_routes {
            if let Some(parent_routes) = &self.allowed_routes {
                let parent_set: HashSet<_> = parent_routes.iter().collect();
                let child_set: HashSet<_> = child_routes.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // allowed_methods: child must be subset of parent
        if let Some(child_methods) = &other.allowed_methods {
            if let Some(parent_methods) = &self.allowed_methods {
                let parent_set: HashSet<_> = parent_methods.iter().collect();
                let child_set: HashSet<_> = child_methods.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // max_request_size: child must be <= parent
        if let Some(child_max) = other.max_request_size {
            if let Some(parent_max) = self.max_request_size {
                if child_max > parent_max {
                    return None;
                }
            } else {
                return None;
            }
        }

        if self == other {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessHostConfig {
    #[serde(default = "default_max_processes")]
    pub max_processes: usize,
    #[serde(default = "default_max_output_buffer")]
    pub max_output_buffer: usize,
    pub allowed_programs: Option<Vec<String>>,
    pub allowed_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessPermissions {
    pub max_processes: usize,
    pub max_output_buffer: usize,
    pub allowed_programs: Option<Vec<String>>,
    pub allowed_paths: Option<Vec<String>>,
}

impl PartialOrd for ProcessPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // max_processes: child must be <= parent
        if self.max_processes < other.max_processes {
            return None;
        }

        // max_output_buffer: child must be <= parent
        if self.max_output_buffer < other.max_output_buffer {
            return None;
        }

        // allowed_programs: parent must be superset of child
        if let Some(child_programs) = &other.allowed_programs {
            if let Some(parent_programs) = &self.allowed_programs {
                let parent_set: HashSet<_> = parent_programs.iter().collect();
                let child_set: HashSet<_> = child_programs.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // allowed_paths: same logic
        if let Some(child_paths) = &other.allowed_paths {
            if let Some(parent_paths) = &self.allowed_paths {
                let parent_set: HashSet<_> = parent_paths.iter().collect();
                let child_set: HashSet<_> = child_paths.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        if self == other {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentHandlerConfig {
    /// Optional allowlist of environment variable names that can be accessed
    pub allowed_vars: Option<Vec<String>>,
    /// Optional denylist of environment variable names that cannot be accessed
    pub denied_vars: Option<Vec<String>>,
    /// Whether to allow listing all environment variables (default: false for security)
    #[serde(default)]
    pub allow_list_all: bool,
    /// Optional prefix filter - only allow vars starting with these prefixes
    pub allowed_prefixes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentPermissions {
    /// Optional allowlist of environment variable names that can be accessed
    pub allowed_vars: Option<Vec<String>>,
    /// Optional denylist of environment variable names that cannot be accessed
    pub denied_vars: Option<Vec<String>>,
    /// Whether to allow listing all environment variables (default: false for security)
    pub allow_list_all: bool,
    /// Optional prefix filter - only allow vars starting with these prefixes
    pub allowed_prefixes: Option<Vec<String>>,
}

impl PartialOrd for EnvironmentPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // allowed_vars: child must be subset of parent
        if let Some(child_vars) = &other.allowed_vars {
            if let Some(parent_vars) = &self.allowed_vars {
                let parent_set: HashSet<_> = parent_vars.iter().collect();
                let child_set: HashSet<_> = child_vars.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // denied_vars: parent must be superset of child
        if let Some(child_denied) = &other.denied_vars {
            if let Some(parent_denied) = &self.denied_vars {
                let parent_set: HashSet<_> = parent_denied.iter().collect();
                let child_set: HashSet<_> = child_denied.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // allow_list_all: parent must be >= child
        if self.allow_list_all < other.allow_list_all {
            return None;
        }

        // allowed_prefixes: child must be subset of parent
        if let Some(child_prefixes) = &other.allowed_prefixes {
            if let Some(parent_prefixes) = &self.allowed_prefixes {
                let parent_set: HashSet<_> = parent_prefixes.iter().collect();
                let child_set: HashSet<_> = child_prefixes.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        if self == other {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater)
        }
    }
}

impl EnvironmentHandlerConfig {
    pub fn is_variable_allowed(&self, var_name: &str) -> bool {
        // Check denied list first
        if let Some(denied) = &self.denied_vars {
            if denied.contains(&var_name.to_string()) {
                return false;
            }
        }

        // Check allowed list
        if let Some(allowed) = &self.allowed_vars {
            return allowed.contains(&var_name.to_string());
        }

        // Check allowed prefixes
        if let Some(prefixes) = &self.allowed_prefixes {
            return prefixes.iter().any(|prefix| var_name.starts_with(prefix));
        }

        // If no restrictions are configured, allow all except denied
        self.denied_vars.is_none()
            || !self
                .denied_vars
                .as_ref()
                .unwrap()
                .contains(&var_name.to_string())
    }
}

fn default_max_processes() -> usize {
    // Default to 10 processes per actor
    10
}

fn default_max_output_buffer() -> usize {
    // Default to 1MB output buffer
    1024 * 1024
}

impl PartialOrd for ProcessHostConfig {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // max_processes: child must be <= parent
        if self.max_processes < other.max_processes {
            return None;
        }

        // max_output_buffer: child must be <= parent
        if self.max_output_buffer < other.max_output_buffer {
            return None;
        }

        // allowed_programs: parent must be superset of child
        if let Some(child_programs) = &other.allowed_programs {
            if let Some(parent_programs) = &self.allowed_programs {
                let parent_set: HashSet<_> = parent_programs.iter().collect();
                let child_set: HashSet<_> = child_programs.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // allowed_paths: same logic
        if let Some(child_paths) = &other.allowed_paths {
            if let Some(parent_paths) = &self.allowed_paths {
                let parent_set: HashSet<_> = parent_paths.iter().collect();
                let child_set: HashSet<_> = child_paths.iter().collect();
                if !parent_set.is_superset(&child_set) {
                    return None;
                }
            } else {
                return None;
            }
        }

        if self == other {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RandomHandlerConfig {
    /// Optional fixed seed for reproducible random numbers (useful for testing)
    pub seed: Option<u64>,
    /// Maximum number of bytes that can be generated in a single call (default: 1MB)
    #[serde(default = "default_max_random_bytes")]
    pub max_bytes: usize,
    /// Maximum number for random integer generation (default: u64::MAX)
    #[serde(default = "default_max_random_int")]
    pub max_int: u64,
    /// Whether to allow cryptographically secure random numbers (default: false)
    #[serde(default)]
    pub allow_crypto_secure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RandomPermissions {
    /// Maximum number of bytes that can be generated in a single call
    pub max_bytes: usize,
    /// Maximum number for random integer generation
    pub max_int: u64,
    /// Whether to allow cryptographically secure random numbers
    pub allow_crypto_secure: bool,
}

impl PartialOrd for RandomPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // max_bytes: child must be <= parent
        if self.max_bytes < other.max_bytes {
            return None;
        }

        // max_int: child must be <= parent
        if self.max_int < other.max_int {
            return None;
        }

        // allow_crypto_secure: parent must be >= child
        if self.allow_crypto_secure < other.allow_crypto_secure {
            return None;
        }

        if self == other {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater)
        }
    }
}

fn default_max_random_bytes() -> usize {
    1024 * 1024 // 1MB
}

fn default_max_random_int() -> u64 {
    // I don't know why but toml serialization thinks u64::MAX is too large
    // I found this: https://github.com/anoma/anoma/pull/488#r723982322
    // that says that toml cannot handle values larger than i64::MAX - 1
    //
    // sorry if you run into this
    9223372036854775807 // i64::MAX - 1
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
    pub async fn load_init_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
        match &self.init_state {
            Some(reference) => {
                let data = resolve_reference(reference).await?;
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
    pub fn into_fixed_bytes(self) -> Result<Vec<u8>, anyhow::Error> {
        debug!("Serializing manifest config to fixed bytes");
        debug!("Manifest config: {:?}", self);
        let serialized = toml::to_string(&self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize manifest: {}", e))?;
        debug!("Serialized manifest config: {}", serialized);
        Ok(serialized.into_bytes())
    }

    pub fn save_chain(&self) -> bool {
        self.save_chain.unwrap_or(false)
    }
}
