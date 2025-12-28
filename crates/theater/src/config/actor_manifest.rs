use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Display;
use std::path::PathBuf;
use tracing::debug;

use crate::utils::resolve_reference;
use crate::utils::template::substitute_variables;

use super::inheritance::{is_default_permission_policy, HandlerPermissionPolicy};
use super::permissions::HandlerPermission;

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
    #[serde(default, skip_serializing_if = "is_default_permission_policy")]
    pub permission_policy: HandlerPermissionPolicy,
    #[serde(default)]
    pub init_state: Option<String>,
    #[serde(default, rename = "handler")]
    pub handlers: Vec<HandlerConfig>,
}

impl Display for ManifestConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ManifestConfig(name: {}, version: {}, component: {}, description: {:?}, long_description: {:?}, save_chain: {:?}, permission_policy: {:?}, init_state: {:?}, handlers: {:?})",
            self.name,
            self.version,
            self.component,
            self.description,
            self.long_description,
            self.save_chain,
            self.permission_policy,
            self.init_state,
            self.handlers
        )
    }
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
#[serde(tag = "type")]
pub enum HandlerConfig {
    #[serde(rename = "message-server")]
    MessageServer {
        #[serde(flatten)]
        config: MessageServerConfig,
    },
    #[serde(rename = "filesystem")]
    FileSystem {
        #[serde(flatten)]
        config: FileSystemHandlerConfig,
    },
    #[serde(rename = "http-client")]
    HttpClient {
        #[serde(flatten)]
        config: HttpClientHandlerConfig,
    },
    #[serde(rename = "http-framework")]
    HttpFramework {
        #[serde(flatten)]
        config: HttpFrameworkHandlerConfig,
    },
    #[serde(rename = "runtime")]
    Runtime {
        #[serde(flatten)]
        config: RuntimeHostConfig,
    },
    #[serde(rename = "supervisor")]
    Supervisor {
        #[serde(flatten)]
        config: SupervisorHostConfig,
    },
    #[serde(rename = "store")]
    Store {
        #[serde(flatten)]
        config: StoreHandlerConfig,
    },
    #[serde(rename = "timing")]
    Timing {
        #[serde(flatten)]
        config: TimingHostConfig,
    },
    #[serde(rename = "process")]
    Process {
        #[serde(flatten)]
        config: ProcessHostConfig,
    },
    #[serde(rename = "environment")]
    Environment {
        #[serde(flatten)]
        config: EnvironmentHandlerConfig,
    },
    #[serde(rename = "random")]
    Random {
        #[serde(flatten)]
        config: RandomHandlerConfig,
    },
    #[serde(rename = "wasi-http")]
    WasiHttp {
        #[serde(flatten)]
        config: WasiHttpHandlerConfig,
    },
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

/// Configuration for the WASI HTTP handler
/// This handler provides both incoming (server) and outgoing (client) HTTP capabilities
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WasiHttpHandlerConfig {
    /// Port to listen on for incoming HTTP requests
    /// If None, no incoming handler server will be started
    #[serde(default)]
    pub port: Option<u16>,
    /// Host to bind to for incoming requests (default: 127.0.0.1)
    #[serde(default = "default_wasi_http_host")]
    pub host: String,
}

fn default_wasi_http_host() -> String {
    "127.0.0.1".to_string()
}

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
    /// use theater::ManifestConfig;
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

    /// Loads a manifest from a string with variable substitution support.
    ///
    /// This is the core method that implements the two-stage loading process:
    /// 1. Extract and resolve the init_state field
    /// 2. Merge with any override state
    /// 3. Perform variable substitution on the raw TOML
    /// 4. Parse the substituted TOML into ManifestConfig
    ///
    /// ## Arguments
    ///
    /// * `content` - Raw TOML content as string
    /// * `override_state` - Optional override state to merge with init_state
    ///
    /// ## Returns
    ///
    /// * `Ok(ManifestConfig)` - The successfully parsed configuration with variables substituted
    /// * `Err(anyhow::Error)` - If initial state cannot be resolved, variable substitution fails,
    ///   or the resulting TOML cannot be parsed
    pub async fn resolve_starting_info(
        content: &str,
        override_state: Option<Value>,
    ) -> anyhow::Result<(Self, Option<Value>)> {
        debug!("Loading manifest with variable substitution");
        debug!("Raw manifest content: {}", content);
        debug!("Override state: {:?}", override_state);

        // Step 1: Extract init_state field from raw TOML
        let init_state_ref = Self::extract_init_state_reference(content)
            .map_err(|e| anyhow::anyhow!("Failed to extract init_state reference: {}", e))?;

        // Step 2: Resolve init_state if present
        let resolved_init_state = if let Some(reference) = init_state_ref {
            debug!("Resolving init_state reference: {}", reference);
            let data = resolve_reference(&reference).await.map_err(|e| {
                anyhow::anyhow!(
                    "Failed to resolve init_state reference '{}': {}",
                    reference,
                    e
                )
            })?;
            let json_value: Value = serde_json::from_slice(&data).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse init_state JSON from reference '{}': {}",
                    reference,
                    e
                )
            })?;
            Some(json_value)
        } else {
            None
        };

        // Step 3: Merge resolved init_state with override_state
        let final_state = Self::merge_states(resolved_init_state, override_state).map_err(|e| {
            anyhow::anyhow!("Failed to merge init_state with override_state: {}", e)
        })?;

        // Step 4: Perform variable substitution if we have state
        let substituted_content = if let Some(state) = &final_state {
            debug!("Performing variable substitution");
            substitute_variables(content, state)
                .map_err(|e| anyhow::anyhow!("Variable substitution failed: {}", e))?
        } else {
            debug!("No state available, skipping variable substitution");
            content.to_string()
        };

        // Step 5: Parse the substituted TOML
        debug!("Parsing substituted manifest TOML");
        let config: ManifestConfig = toml::from_str(&substituted_content).map_err(|e| {
            anyhow::anyhow!("Failed to parse manifest TOML after substitution: {}", e)
        })?;

        debug!("Successfully parsed manifest configuration");
        debug!("Manifest: {}", config);
        debug!("Final state after merging: {:?}", final_state);
        Ok((config, final_state))
    }

    /// Extract the init_state field value from raw TOML without full parsing.
    fn extract_init_state_reference(content: &str) -> anyhow::Result<Option<String>> {
        // Parse as a generic TOML value first
        let value: toml::Value = toml::from_str(content)?;

        // Extract init_state if present
        if let Some(init_state_value) = value.get("init_state") {
            if let Some(reference) = init_state_value.as_str() {
                Ok(Some(reference.to_string()))
            } else {
                Err(anyhow::anyhow!("init_state field must be a string"))
            }
        } else {
            Ok(None)
        }
    }

    /// Merge resolved init_state with override state.
    fn merge_states(
        init_state: Option<Value>,
        override_state: Option<Value>,
    ) -> anyhow::Result<Option<Value>> {
        match (init_state, override_state) {
            (None, None) => Ok(None),
            (Some(state), None) => Ok(Some(state)),
            (None, Some(ref state)) => Ok(Some(state.clone())),
            (Some(mut init), Some(ref override_val)) => {
                if let (Value::Object(ref mut init_map), Value::Object(override_map)) =
                    (&mut init, override_val)
                {
                    // Merge override values into init state
                    for (key, value) in override_map {
                        init_map.insert(key.clone(), value.clone());
                    }
                    Ok(Some(init))
                } else {
                    // If either isn't an object, just use the override
                    debug!("Either init_state or override_state is not an object, using override");
                    Ok(Some(override_val.clone()))
                }
            }
        }
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
    /// use theater::ManifestConfig;
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
        tracing::info!("Parsing manifest TOML content: {}", content);

        let config: ManifestConfig = match toml::from_str(content) {
            Ok(config) => {
                tracing::info!("Successfully parsed manifest TOML");
                config
            }
            Err(e) => {
                tracing::error!("Failed to parse manifest TOML: {}", e);
                return Err(e.into());
            }
        };

        // Debug logging to trace permission parsing
        tracing::info!(
            "Parsed manifest permission_policy: {:?}",
            config.permission_policy
        );
        tracing::info!(
            "Parsed manifest file_system inheritance: {:?}",
            config.permission_policy.file_system
        );

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
    /// use theater::ManifestConfig;
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
    /// # use theater::ManifestConfig;
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
    /// # use theater::ManifestConfig;
    /// # async fn example(config: &ManifestConfig) -> anyhow::Result<()> {
    /// if let Some(state_data) = config.load_init_state().await? {
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
    /// # use theater::ManifestConfig;
    /// # fn example(config: ManifestConfig) {
    /// let bytes = config.into_fixed_bytes();
    /// println!("Serialized manifest: {} bytes", bytes.unwrap().len());
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

    /// Calculate effective permissions based on parent permissions and this manifest's policy
    pub fn calculate_effective_permissions(
        &self,
        parent_permissions: &HandlerPermission,
    ) -> HandlerPermission {
        tracing::info!(
            "Calculating effective permissions from parent: {:?}",
            parent_permissions
        );
        tracing::info!("Using permission policy: {:?}", self.permission_policy);

        let effective =
            HandlerPermission::calculate_effective(parent_permissions, &self.permission_policy);

        tracing::info!("Calculated effective permissions: {:?}", effective);
        tracing::info!(
            "Effective filesystem permissions: {:?}",
            effective.file_system
        );

        effective
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::permissions::*;

    #[test]
    fn test_manifest_permission_policy_parsing() {
        // Test the correct permission_policy structure
        let toml_content = r#"
            name = "test-actor"
            version = "0.1.0"
            component = "test.wasm"
            
            [permission_policy.file_system]
            type = "restrict"
            [permission_policy.file_system.config]
            read = true
            write = true
            execute = false
            allowed_paths = ["/tmp/test"]
        "#;

        let manifest = ManifestConfig::from_str(toml_content).unwrap();

        // Check that the permission policy was parsed correctly
        match &manifest.permission_policy.file_system {
            crate::config::inheritance::HandlerInheritance::Restrict(perms) => {
                assert_eq!(perms.read, true);
                assert_eq!(perms.write, true);
                assert_eq!(perms.execute, false);
                assert_eq!(perms.allowed_paths, Some(vec!["/tmp/test".to_string()]));
            }
            _ => panic!("Expected Restrict variant with FileSystemPermissions"),
        }
    }

    #[test]
    fn test_manifest_wrong_permissions_structure() {
        // Test what happens with your original structure
        let toml_content = r#"
            name = "test-actor"
            version = "0.1.0"
            component = "test.wasm"
            
            [permissions.file_system]
            read = true
            write = true
            execute = false
            allowed_paths = ["/tmp/test"]
        "#;

        let manifest = ManifestConfig::from_str(toml_content).unwrap();

        // This should result in default (Inherit) since the structure is wrong
        match &manifest.permission_policy.file_system {
            crate::config::inheritance::HandlerInheritance::Inherit => {
                println!("As expected, wrong structure results in Inherit");
            }
            other => panic!("Expected Inherit, got: {:?}", other),
        }
    }
}
