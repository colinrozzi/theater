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

/// Validates that handler configurations don't exceed effective permissions
pub fn validate_manifest_permissions(
    manifest: &ManifestConfig,
    effective_permissions: &HandlerPermission,
) -> PermissionResult<()> {
    for handler_config in &manifest.handlers {
        match handler_config {
            HandlerConfig::FileSystem { config } => {
                validate_filesystem_config(config, &effective_permissions.file_system)?;
            }
            HandlerConfig::HttpClient { config } => {
                validate_http_client_config(config, &effective_permissions.http_client)?;
            }
            HandlerConfig::HttpFramework { config } => {
                validate_http_framework_config(config, &effective_permissions.http_framework)?;
            }
            HandlerConfig::Process { config } => {
                validate_process_config(config, &effective_permissions.process)?;
            }
            HandlerConfig::Environment { config } => {
                validate_environment_config(config, &effective_permissions.environment)?;
            }
            HandlerConfig::Random { config } => {
                validate_random_config(config, &effective_permissions.random)?;
            }
            HandlerConfig::Timing { config } => {
                validate_timing_config(config, &effective_permissions.timing)?;
            }
            HandlerConfig::MessageServer { .. } => {
                if effective_permissions.message_server.is_none() {
                    return Err(PermissionError::HandlerNotPermitted {
                        handler_type: "message-server".to_string(),
                    });
                }
            }
            HandlerConfig::Runtime { .. } => {
                if effective_permissions.runtime.is_none() {
                    return Err(PermissionError::HandlerNotPermitted {
                        handler_type: "runtime".to_string(),
                    });
                }
            }
            HandlerConfig::Supervisor { .. } => {
                if effective_permissions.supervisor.is_none() {
                    return Err(PermissionError::HandlerNotPermitted {
                        handler_type: "supervisor".to_string(),
                    });
                }
            }
            HandlerConfig::Store { .. } => {
                if effective_permissions.store.is_none() {
                    return Err(PermissionError::HandlerNotPermitted {
                        handler_type: "store".to_string(),
                    });
                }
            }
            HandlerConfig::WasiHttp { .. } => {
                // WASI HTTP handler is allowed by default
                // It provides both incoming (server) and outgoing (client) capabilities
                // Permission enforcement for HTTP operations happens at a finer grain
            }
            HandlerConfig::Replay { .. } => {
                // Replay handler is always allowed - it's for debugging/testing
                // The replay handler replays recorded event chains
            }
        }
    }
    Ok(())
}

fn validate_filesystem_config(
    config: &FileSystemHandlerConfig,
    permissions: &Option<FileSystemPermissions>,
) -> PermissionResult<()> {
    let perms = permissions
        .as_ref()
        .ok_or_else(|| PermissionError::HandlerNotPermitted {
            handler_type: "filesystem".to_string(),
        })?;

    // Check if requested path is allowed
    if let Some(config_path) = &config.path {
        if let Some(allowed_paths) = &perms.allowed_paths {
            let path_allowed = allowed_paths
                .iter()
                .any(|allowed| config_path.starts_with(allowed));
            if !path_allowed {
                return Err(PermissionError::PathNotAllowed {
                    path: config_path.to_string_lossy().to_string(),
                    allowed_paths: allowed_paths.clone(),
                });
            }
        }
    }

    // Check if new_dir is allowed
    if config.new_dir == Some(true) && perms.new_dir != Some(true) {
        return Err(PermissionError::ConfigExceedsPermissions {
            reason: "new_dir capability not granted".to_string(),
        });
    }

    // Check allowed commands
    if let Some(config_commands) = &config.allowed_commands {
        if let Some(allowed_commands) = &perms.allowed_commands {
            for command in config_commands {
                if !allowed_commands.contains(command) {
                    return Err(PermissionError::CommandNotAllowed {
                        command: command.clone(),
                        allowed_commands: allowed_commands.clone(),
                    });
                }
            }
        }
    }

    Ok(())
}

fn validate_http_client_config(
    _config: &HttpClientHandlerConfig,
    permissions: &Option<HttpClientPermissions>,
) -> PermissionResult<()> {
    if permissions.is_none() {
        return Err(PermissionError::HandlerNotPermitted {
            handler_type: "http-client".to_string(),
        });
    }
    // HTTP client config is currently empty, but we validate that the capability exists
    Ok(())
}

fn validate_http_framework_config(
    _config: &HttpFrameworkHandlerConfig,
    permissions: &Option<HttpFrameworkPermissions>,
) -> PermissionResult<()> {
    if permissions.is_none() {
        return Err(PermissionError::HandlerNotPermitted {
            handler_type: "http-framework".to_string(),
        });
    }
    Ok(())
}

fn validate_process_config(
    config: &ProcessHostConfig,
    permissions: &Option<ProcessPermissions>,
) -> PermissionResult<()> {
    let perms = permissions
        .as_ref()
        .ok_or_else(|| PermissionError::HandlerNotPermitted {
            handler_type: "process".to_string(),
        })?;

    // Check max_processes limit
    if config.max_processes > perms.max_processes {
        return Err(PermissionError::ResourceLimitExceeded {
            resource: "max_processes".to_string(),
            requested: config.max_processes,
            limit: perms.max_processes,
        });
    }

    // Check max_output_buffer limit
    if config.max_output_buffer > perms.max_output_buffer {
        return Err(PermissionError::ResourceLimitExceeded {
            resource: "max_output_buffer".to_string(),
            requested: config.max_output_buffer,
            limit: perms.max_output_buffer,
        });
    }

    // Check allowed programs
    if let Some(config_programs) = &config.allowed_programs {
        if let Some(allowed_programs) = &perms.allowed_programs {
            for program in config_programs {
                if !allowed_programs.contains(program) {
                    return Err(PermissionError::ConfigExceedsPermissions {
                        reason: format!("Program '{}' not in allowed programs", program),
                    });
                }
            }
        }
    }

    // Check allowed paths
    if let Some(config_paths) = &config.allowed_paths {
        if let Some(allowed_paths) = &perms.allowed_paths {
            for path in config_paths {
                if !allowed_paths.contains(path) {
                    return Err(PermissionError::PathNotAllowed {
                        path: path.clone(),
                        allowed_paths: allowed_paths.clone(),
                    });
                }
            }
        }
    }

    Ok(())
}

fn validate_environment_config(
    config: &EnvironmentHandlerConfig,
    permissions: &Option<EnvironmentPermissions>,
) -> PermissionResult<()> {
    let perms = permissions
        .as_ref()
        .ok_or_else(|| PermissionError::HandlerNotPermitted {
            handler_type: "environment".to_string(),
        })?;

    // Check if allow_list_all is permitted
    if config.allow_list_all && !perms.allow_list_all {
        return Err(PermissionError::ConfigExceedsPermissions {
            reason: "allow_list_all not permitted".to_string(),
        });
    }

    // Check allowed vars are subset of permissions
    if let Some(config_vars) = &config.allowed_vars {
        if let Some(allowed_vars) = &perms.allowed_vars {
            for var in config_vars {
                if !allowed_vars.contains(var) {
                    return Err(PermissionError::EnvVarDenied { var: var.clone() });
                }
            }
        }
    }

    Ok(())
}

fn validate_random_config(
    config: &RandomHandlerConfig,
    permissions: &Option<RandomPermissions>,
) -> PermissionResult<()> {
    let perms = permissions
        .as_ref()
        .ok_or_else(|| PermissionError::HandlerNotPermitted {
            handler_type: "random".to_string(),
        })?;

    // Check max_bytes limit
    if config.max_bytes > perms.max_bytes {
        return Err(PermissionError::ResourceLimitExceeded {
            resource: "max_bytes".to_string(),
            requested: config.max_bytes,
            limit: perms.max_bytes,
        });
    }

    // Check max_int limit
    if config.max_int > perms.max_int {
        return Err(PermissionError::ResourceLimitExceeded {
            resource: "max_int".to_string(),
            requested: config.max_int as usize,
            limit: perms.max_int as usize,
        });
    }

    // Check crypto_secure capability
    if config.allow_crypto_secure && !perms.allow_crypto_secure {
        return Err(PermissionError::ConfigExceedsPermissions {
            reason: "crypto_secure random generation not permitted".to_string(),
        });
    }

    Ok(())
}

fn validate_timing_config(
    config: &TimingHostConfig,
    permissions: &Option<TimingPermissions>,
) -> PermissionResult<()> {
    let perms = permissions
        .as_ref()
        .ok_or_else(|| PermissionError::HandlerNotPermitted {
            handler_type: "timing".to_string(),
        })?;

    // Check max_sleep_duration limit
    if config.max_sleep_duration > perms.max_sleep_duration {
        return Err(PermissionError::ResourceLimitExceeded {
            resource: "max_sleep_duration".to_string(),
            requested: config.max_sleep_duration as usize,
            limit: perms.max_sleep_duration as usize,
        });
    }

    // Check min_sleep_duration requirement
    if config.min_sleep_duration < perms.min_sleep_duration {
        return Err(PermissionError::ConfigExceedsPermissions {
            reason: format!(
                "min_sleep_duration {} below required minimum {}",
                config.min_sleep_duration, perms.min_sleep_duration
            ),
        });
    }

    Ok(())
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
