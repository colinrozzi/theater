use thiserror::Error;

/// Main error type for the Theater CLI
#[derive(Error, Debug)]
pub enum CliError {
    /// Actor-related errors
    #[error("Actor '{actor_id}' not found")]
    ActorNotFound { actor_id: String },

    #[error("Actor '{actor_id}' failed to start: {reason}")]
    ActorStartFailed { actor_id: String, reason: String },

    #[error("Actor '{actor_id}' is not running")]
    ActorNotRunning { actor_id: String },

    #[error("Actor '{actor_id}' had an error: {reason}")]
    ActorError { actor_id: String, reason: String },

    /// Project and build errors
    #[error("Invalid project directory: {path}")]
    InvalidProjectDirectory { path: String },

    #[error("Build failed: {output}")]
    BuildFailed { output: String },

    #[error("Missing required tool: {tool}. Please install it with: {install_command}")]
    MissingTool {
        tool: String,
        install_command: String,
    },

    /// Manifest and configuration errors
    #[error("Invalid manifest file: {reason}")]
    InvalidManifest { reason: String },

    #[error("Configuration error: {reason}")]
    ConfigError { reason: String },

    /// Template errors
    #[error("Template '{template}' not found. Available templates: {available}")]
    TemplateNotFound { template: String, available: String },

    #[error("Template error: {reason}")]
    TemplateError { reason: String },

    /// I/O and filesystem errors
    #[error("File operation failed: {operation} on {path}: {source}")]
    FileOperationFailed {
        operation: String,
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Permission denied: {operation} on {path}")]
    PermissionDenied { operation: String, path: String },

    /// Validation errors
    #[error("Invalid input: {field} = '{value}'. {suggestion}")]
    InvalidInput {
        field: String,
        value: String,
        suggestion: String,
    },

    #[error("Validation failed: {reason}")]
    ValidationError { reason: String },

    /// Server/runtime errors
    #[error("Server error: {message}")]
    ServerError { message: String },

    /// Cancellation errors
    #[error("Operation was cancelled by user")]
    OperationCancelled,

    /// Generic wrapper for other errors
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl CliError {
    /// Create an actor not found error
    pub fn actor_not_found(actor_id: impl Into<String>) -> Self {
        Self::ActorNotFound {
            actor_id: actor_id.into(),
        }
    }

    /// Create a build failed error
    pub fn build_failed(output: impl Into<String>) -> Self {
        Self::BuildFailed {
            output: output.into(),
        }
    }

    /// Create an invalid manifest error
    pub fn invalid_manifest(reason: impl Into<String>) -> Self {
        Self::InvalidManifest {
            reason: reason.into(),
        }
    }

    /// Create a template not found error
    pub fn template_not_found(template: impl Into<String>, available: Vec<String>) -> Self {
        Self::TemplateNotFound {
            template: template.into(),
            available: available.join(", "),
        }
    }

    /// Create a file operation failed error
    pub fn file_operation_failed(
        operation: impl Into<String>,
        path: impl Into<String>,
        source: std::io::Error,
    ) -> Self {
        Self::FileOperationFailed {
            operation: operation.into(),
            path: path.into(),
            source,
        }
    }

    /// Create an invalid input error with helpful suggestions
    pub fn invalid_input(
        field: impl Into<String>,
        value: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::InvalidInput {
            field: field.into(),
            value: value.into(),
            suggestion: suggestion.into(),
        }
    }

    /// Create a server/runtime error
    pub fn server_error(message: impl Into<String>) -> Self {
        Self::ServerError {
            message: message.into(),
        }
    }

    /// Get a user-friendly error message with potential solutions
    pub fn user_message(&self) -> String {
        match self {
            Self::ActorNotFound { actor_id } => {
                format!(
                    "Actor '{}' was not found.\n\n\
                    Possible solutions:\n\
                    • Check the actor ID is correct\n\
                    • Start the actor with: theater start <manifest>",
                    actor_id
                )
            }
            Self::BuildFailed { output } => {
                format!(
                    "Build failed.\n\n\
                    Build output:\n{}\n\n\
                    Possible solutions:\n\
                    • Check for compilation errors in your Rust code\n\
                    • Ensure all dependencies are available\n\
                    • Try a clean build with: theater build --clean",
                    output
                )
            }
            Self::MissingTool {
                tool,
                install_command,
            } => {
                format!(
                    "Required tool '{}' is not installed.\n\n\
                    Install it with:\n  {}",
                    tool, install_command
                )
            }
            Self::TemplateNotFound {
                template,
                available,
            } => {
                format!(
                    "Template '{}' was not found.\n\n\
                    Available templates: {}\n\n\
                    Use one of the available templates or check your template configuration.",
                    template, available
                )
            }
            Self::InvalidInput {
                field,
                value,
                suggestion,
            } => {
                format!("Invalid {}: '{}'\n\n{}", field, value, suggestion)
            }
            _ => self.to_string(),
        }
    }

    /// Check if this error suggests the user should retry
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::ServerError { .. })
    }

    /// Get the error category for metrics/logging
    pub fn category(&self) -> &'static str {
        match self {
            Self::ActorNotFound { .. }
            | Self::ActorStartFailed { .. }
            | Self::ActorNotRunning { .. }
            | Self::ActorError { .. } => "actor",
            Self::InvalidProjectDirectory { .. }
            | Self::BuildFailed { .. }
            | Self::MissingTool { .. } => "build",
            Self::InvalidManifest { .. } | Self::ConfigError { .. } => "config",
            Self::TemplateNotFound { .. } | Self::TemplateError { .. } => "template",
            Self::FileOperationFailed { .. } | Self::PermissionDenied { .. } => "filesystem",
            Self::InvalidInput { .. } | Self::ValidationError { .. } => "validation",
            Self::ServerError { .. } => "server",
            Self::OperationCancelled => "cancellation",
            Self::Internal(_) | Self::Serialization(_) | Self::Io(_) => "internal",
        }
    }
}

/// Result type alias for CLI operations
pub type CliResult<T> = Result<T, CliError>;
