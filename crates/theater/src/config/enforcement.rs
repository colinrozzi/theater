use crate::config::actor_manifest::*;
use crate::config::permissions::*;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PermissionError {
    #[error("Handler configuration exceeds granted permissions: {reason}")]
    ConfigExceedsPermissions { reason: String },

    #[error("Operation denied: {operation} not permitted by {permission_type} permissions")]
    OperationDenied {
        operation: String,
        permission_type: String,
    },

    #[error("Handler type '{handler_type}' not permitted by effective permissions")]
    HandlerNotPermitted { handler_type: String },

    #[error("Path '{path}' not in allowed paths: {allowed_paths:?}")]
    PathNotAllowed {
        path: String,
        allowed_paths: Vec<String>,
    },

    #[error("Command '{command}' not in allowed commands: {allowed_commands:?}")]
    CommandNotAllowed {
        command: String,
        allowed_commands: Vec<String>,
    },

    #[error("Host '{host}' not in allowed hosts: {allowed_hosts:?}")]
    HostNotAllowed {
        host: String,
        allowed_hosts: Vec<String>,
    },

    #[error("Method '{method}' not in allowed methods: {allowed_methods:?}")]
    MethodNotAllowed {
        method: String,
        allowed_methods: Vec<String>,
    },

    #[error("Resource limit exceeded: {resource} = {requested} > {limit}")]
    ResourceLimitExceeded {
        resource: String,
        requested: usize,
        limit: usize,
    },

    #[error("Environment variable '{var}' access denied")]
    EnvVarDenied { var: String },
}

pub type PermissionResult<T> = Result<T, PermissionError>;

/// Validates that a spawned actor's `[[handler]]` configs don't exceed its
/// effective permissions (its spawner's set narrowed by its own policy).
///
/// Only the configs that can widen what an actor reaches are checked here:
/// `filesystem`, `http-client` and `process`. Containment in the spawner's own
/// config (a filesystem path inside the spawner's root, and so on) is checked by
/// each handler's `check_child_config`; this checks against the permission set.
/// Every error names the handler and the config key.
pub fn validate_manifest_permissions(
    manifest: &ManifestConfig,
    effective_permissions: &HandlerPermission,
) -> PermissionResult<()> {
    for handler_config in &manifest.handlers {
        let handler = handler_config.type_name();
        let exceeds = |reason: String| PermissionError::ConfigExceedsPermissions {
            reason: format!("[[handler]] '{}': {}", handler, reason),
        };
        match handler {
            "filesystem" => {
                let config: FileSystemHandlerConfig = typed(handler_config)?;
                let perms = effective_permissions
                    .file_system
                    .as_ref()
                    .ok_or_else(|| exceeds("filesystem is not granted".to_string()))?;
                if config.new_dir == Some(true) && perms.new_dir != Some(true) {
                    return Err(exceeds("new_dir = true is not granted".to_string()));
                }
                if let (Some(asked), Some(granted)) =
                    (&config.allowed_commands, &perms.allowed_commands)
                {
                    if let Some(c) = asked.iter().find(|c| !granted.contains(c)) {
                        return Err(exceeds(format!(
                            "allowed_commands entry '{}' is not granted (granted: {:?})",
                            c, granted
                        )));
                    }
                }
            }
            "http-client" => {
                let config: HttpClientHandlerConfig = typed(handler_config)?;
                let perms = effective_permissions
                    .http_client
                    .as_ref()
                    .ok_or_else(|| exceeds("http-client is not granted".to_string()))?;
                if let Some(granted) = &perms.allowed_hosts {
                    if let Some(h) = config.allowed_hosts.iter().find(|h| !granted.contains(h)) {
                        return Err(exceeds(format!(
                            "allowed_hosts entry '{}' is not granted (granted: {:?})",
                            h, granted
                        )));
                    }
                }
            }
            "process" => {
                let config: ProcessLimits = typed(handler_config)?;
                let perms = effective_permissions
                    .process
                    .as_ref()
                    .ok_or_else(|| exceeds("process is not granted".to_string()))?;
                if let Some(granted) = &perms.allowed_programs {
                    match &config.allowed_programs {
                        None => {
                            return Err(exceeds(format!(
                                "allowed_programs is unset (any program) but only {:?} is granted",
                                granted
                            )))
                        }
                        Some(asked) => {
                            let granted_any = granted.iter().any(|g| g == "*");
                            if let Some(p) =
                                asked.iter().find(|p| !granted_any && !granted.contains(p))
                            {
                                return Err(exceeds(format!(
                                    "allowed_programs entry '{}' is not granted (granted: {:?})",
                                    p, granted
                                )));
                            }
                        }
                    }
                }
                if perms.max_processes != usize::MAX {
                    match config.max_processes {
                        Some(n) if n <= perms.max_processes => {}
                        asked => {
                            return Err(exceeds(format!(
                                "max_processes {} exceeds the granted {}",
                                asked.map_or("unset (unlimited)".to_string(), |n| n.to_string()),
                                perms.max_processes
                            )))
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Check that `path`, a child's config value for `key`, is inside the spawner's
/// `root`. Both are resolved first ([`real_path`]), so a symlink out of the root
/// or a `..` is refused. For handlers' `check_child_config`.
pub fn check_path_within(
    key: &str,
    path: &std::path::Path,
    root: &std::path::Path,
) -> Result<(), String> {
    // The root may not exist yet (a store makes its directory on first use).
    let real_root = std::fs::canonicalize(root)
        .or_else(|_| real_path(root))
        .map_err(|e| format!("the spawner's {} {}", key, e))?;
    let real = real_path(path).map_err(|e| format!("{} {}", key, e))?;
    if !real.starts_with(&real_root) {
        return Err(format!(
            "{} {:?} is outside the spawner's {:?}",
            key, path, real_root
        ));
    }
    Ok(())
}

/// Where `path` really is: its longest existing ancestor with symlinks
/// resolved, plus the components that don't exist yet. A `..` anywhere is
/// refused, so the part that doesn't exist yet can't climb back out.
fn real_path(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!("{:?} must not contain '..'", path));
    }
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(real) => return Ok(rest.iter().rev().fold(real, |p, c| p.join(c))),
            Err(_) => match (existing.file_name(), existing.parent()) {
                (Some(name), Some(parent)) => {
                    rest.push(name.to_owned());
                    existing = if parent.as_os_str().is_empty() {
                        std::path::PathBuf::from(".")
                    } else {
                        parent.to_path_buf()
                    };
                }
                _ => return Err(format!("{:?} cannot be resolved", path)),
            },
        }
    }
}

/// Parse a handler's raw config; a malformed one is refused at spawn.
fn typed<T: serde::de::DeserializeOwned>(hc: &HandlerConfig) -> PermissionResult<T> {
    hc.parse()
        .map_err(|e| PermissionError::ConfigExceedsPermissions {
            reason: format!("[[handler]] '{}': invalid config: {}", hc.type_name(), e),
        })
}

/// The limits in a `process` handler config (the full config lives in
/// theater-handler-process; the other keys don't widen anything).
#[derive(serde::Deserialize)]
struct ProcessLimits {
    #[serde(default)]
    allowed_programs: Option<Vec<String>>,
    #[serde(default)]
    max_processes: Option<usize>,
}

/// Runtime permission checking utilities
pub struct PermissionChecker;

impl PermissionChecker {
    /// Check if a filesystem operation is allowed
    pub fn check_filesystem_operation(
        permissions: &Option<FileSystemPermissions>,
        operation: &str,
        path: Option<&str>,
        command: Option<&str>,
    ) -> PermissionResult<()> {
        let perms = permissions
            .as_ref()
            .ok_or_else(|| PermissionError::OperationDenied {
                operation: operation.to_string(),
                permission_type: "filesystem".to_string(),
            })?;

        match operation {
            "read" => {
                if !perms.read {
                    return Err(PermissionError::OperationDenied {
                        operation: "read".to_string(),
                        permission_type: "filesystem".to_string(),
                    });
                }
            }
            "write" => {
                if !perms.write {
                    return Err(PermissionError::OperationDenied {
                        operation: "write".to_string(),
                        permission_type: "filesystem".to_string(),
                    });
                }
            }
            "execute" => {
                if !perms.execute {
                    return Err(PermissionError::OperationDenied {
                        operation: "execute".to_string(),
                        permission_type: "filesystem".to_string(),
                    });
                }
            }
            _ => {}
        }

        // Check path restrictions - FAIL CLOSED: require explicit path allowlist
        if let Some(path) = path {
            let allowed_paths =
                perms
                    .allowed_paths
                    .as_ref()
                    .ok_or_else(|| PermissionError::PathNotAllowed {
                        path: path.to_string(),
                        allowed_paths: vec!["<none configured>".to_string()],
                    })?;

            let path_allowed = allowed_paths
                .iter()
                .any(|allowed| path.starts_with(allowed));
            if !path_allowed {
                return Err(PermissionError::PathNotAllowed {
                    path: path.to_string(),
                    allowed_paths: allowed_paths.clone(),
                });
            }
        }

        // Check command restrictions
        if let Some(command) = command {
            if let Some(allowed_commands) = &perms.allowed_commands {
                if !allowed_commands.contains(&command.to_string()) {
                    return Err(PermissionError::CommandNotAllowed {
                        command: command.to_string(),
                        allowed_commands: allowed_commands.clone(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Check if an HTTP operation is allowed
    pub fn check_http_operation(
        permissions: &Option<HttpClientPermissions>,
        method: &str,
        host: &str,
    ) -> PermissionResult<()> {
        let perms = permissions
            .as_ref()
            .ok_or_else(|| PermissionError::OperationDenied {
                operation: format!("{} {}", method, host),
                permission_type: "http_client".to_string(),
            })?;

        // Check method
        if let Some(allowed_methods) = &perms.allowed_methods {
            if !allowed_methods.contains(&method.to_string()) {
                return Err(PermissionError::MethodNotAllowed {
                    method: method.to_string(),
                    allowed_methods: allowed_methods.clone(),
                });
            }
        }

        // Check host
        if let Some(allowed_hosts) = &perms.allowed_hosts {
            if !allowed_hosts.contains(&host.to_string()) {
                return Err(PermissionError::HostNotAllowed {
                    host: host.to_string(),
                    allowed_hosts: allowed_hosts.clone(),
                });
            }
        }

        Ok(())
    }

    /// Check if an environment variable access is allowed
    pub fn check_env_var_access(
        permissions: &Option<EnvironmentPermissions>,
        var_name: &str,
    ) -> PermissionResult<()> {
        let perms = permissions
            .as_ref()
            .ok_or_else(|| PermissionError::OperationDenied {
                operation: format!("access env var {}", var_name),
                permission_type: "environment".to_string(),
            })?;

        // Check denied list first
        if let Some(denied_vars) = &perms.denied_vars {
            if denied_vars.contains(&var_name.to_string()) {
                return Err(PermissionError::EnvVarDenied {
                    var: var_name.to_string(),
                });
            }
        }

        // Check allowed list
        if let Some(allowed_vars) = &perms.allowed_vars {
            if !allowed_vars.contains(&var_name.to_string()) {
                return Err(PermissionError::EnvVarDenied {
                    var: var_name.to_string(),
                });
            }
        }

        // Check prefixes
        if let Some(allowed_prefixes) = &perms.allowed_prefixes {
            let has_allowed_prefix = allowed_prefixes
                .iter()
                .any(|prefix| var_name.starts_with(prefix));
            if !has_allowed_prefix {
                return Err(PermissionError::EnvVarDenied {
                    var: var_name.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Check if a process operation is allowed
    pub fn check_process_operation(
        permissions: &Option<ProcessPermissions>,
        program: &str,
        current_process_count: usize,
    ) -> PermissionResult<()> {
        let perms = permissions
            .as_ref()
            .ok_or_else(|| PermissionError::OperationDenied {
                operation: format!("execute {}", program),
                permission_type: "process".to_string(),
            })?;

        // Check process count limit
        if current_process_count >= perms.max_processes {
            return Err(PermissionError::ResourceLimitExceeded {
                resource: "process_count".to_string(),
                requested: current_process_count + 1,
                limit: perms.max_processes,
            });
        }

        // Check allowed programs
        if let Some(allowed_programs) = &perms.allowed_programs {
            if !allowed_programs.contains(&program.to_string()) {
                return Err(PermissionError::ConfigExceedsPermissions {
                    reason: format!("Program '{}' not allowed", program),
                });
            }
        }

        Ok(())
    }

    /// Check if a random operation is allowed
    pub fn check_random_operation(
        permissions: &Option<RandomPermissions>,
        operation: &str,
        bytes_requested: Option<usize>,
        max_value: Option<u64>,
    ) -> PermissionResult<()> {
        let perms = permissions
            .as_ref()
            .ok_or_else(|| PermissionError::OperationDenied {
                operation: operation.to_string(),
                permission_type: "random".to_string(),
            })?;

        // Check byte limit
        if let Some(bytes) = bytes_requested {
            if bytes > perms.max_bytes {
                return Err(PermissionError::ResourceLimitExceeded {
                    resource: "random_bytes".to_string(),
                    requested: bytes,
                    limit: perms.max_bytes,
                });
            }
        }

        // Check max value limit
        if let Some(max_val) = max_value {
            if max_val > perms.max_int {
                return Err(PermissionError::ResourceLimitExceeded {
                    resource: "random_max_int".to_string(),
                    requested: max_val as usize,
                    limit: perms.max_int as usize,
                });
            }
        }

        Ok(())
    }

    /// Check if a timing operation is allowed
    pub fn check_timing_operation(
        permissions: &Option<TimingPermissions>,
        operation: &str,
        duration_ms: u64,
    ) -> PermissionResult<()> {
        let perms = permissions
            .as_ref()
            .ok_or_else(|| PermissionError::OperationDenied {
                operation: operation.to_string(),
                permission_type: "timing".to_string(),
            })?;

        // Check max duration limit
        if duration_ms > perms.max_sleep_duration {
            return Err(PermissionError::ResourceLimitExceeded {
                resource: "sleep_duration".to_string(),
                requested: duration_ms as usize,
                limit: perms.max_sleep_duration as usize,
            });
        }

        // Check min duration limit
        if duration_ms < perms.min_sleep_duration {
            return Err(PermissionError::ResourceLimitExceeded {
                resource: "min_sleep_duration".to_string(),
                requested: duration_ms as usize,
                limit: perms.min_sleep_duration as usize,
            });
        }

        Ok(())
    }
}
