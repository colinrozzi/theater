use std::net::SocketAddr;
use thiserror::Error;

/// Main error type for the Theater CLI
#[derive(Error, Debug)]
pub enum CliError {
    /// Connection-related errors
    #[error("Failed to connect to Theater server at {address}")]
    ConnectionFailed {
        address: SocketAddr,
        #[source]
        source: anyhow::Error,
    },

    #[error("Connection lost to Theater server")]
    ConnectionLost,

    #[error("Connection timeout after {timeout}s")]
    ConnectionTimeout { timeout: u64 },

    /// Actor-related errors
    #[error("Actor '{actor_id}' not found")]
    ActorNotFound { actor_id: String },

    #[error("Actor '{actor_id}' failed to start: {reason}")]
    ActorStartFailed { actor_id: String, reason: String },

    #[error("Actor '{actor_id}' is not running")]
    ActorNotRunning { actor_id: String },

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
    #[error("File operation failed: {operation} on {path}")]
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

    /// Server and protocol errors
    #[error("Server error: {message}")]
    ServerError { message: String },

    #[error("Protocol error: {reason}")]
    ProtocolError { reason: String },

    #[error("Unexpected response from server: {response}")]
    UnexpectedResponse { response: String },

    /// Event and monitoring errors
    #[error("Event stream error: {reason}")]
    EventStreamError { reason: String },

    #[error("Event filter error: {filter} is invalid")]
    EventFilterError { filter: String },

    /// Generic wrapper for other errors
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Network and protocol specific errors
    #[error("Network error during {operation}: {source}")]
    NetworkError {
        operation: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Invalid response: {message}")]
    InvalidResponse {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("I/O operation failed: {operation}")]
    IoError {
        operation: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Parse error: {message}")]
    ParseError { message: String },

    #[error("Not implemented: {feature}")]
    NotImplemented { feature: String, message: String },
}

impl CliError {
    /// Create a connection failed error
    pub fn connection_failed(address: SocketAddr, source: impl Into<anyhow::Error>) -> Self {
        Self::ConnectionFailed {
            address,
            source: source.into(),
        }
    }

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

    /// Create an invalid actor ID error
    pub fn invalid_actor_id(actor_id: impl Into<String>) -> Self {
        let actor_id = actor_id.into();
        Self::InvalidInput {
            field: "actor_id".to_string(),
            value: actor_id,
            suggestion:
                "Actor ID must be a valid UUID (e.g., 123e4567-e89b-12d3-a456-426614174000)"
                    .to_string(),
        }
    }

    pub fn operation_timeout(_operation: impl Into<String>, timeout: u64) -> Self {
        Self::ConnectionTimeout { timeout }
    }

    /// Create a not implemented error
    pub fn not_implemented(feature: impl Into<String>, message: impl Into<String>) -> Self {
        Self::NotImplemented {
            feature: feature.into(),
            message: message.into(),
        }
    }

    /// Get a user-friendly error message with potential solutions
    pub fn user_message(&self) -> String {
        match self {
            Self::ConnectionFailed { address, .. } => {
                format!(
                    "Could not connect to Theater server at {}.\n\n\
                    Possible solutions:\n\
                    • Start a Theater server with: theater server\n\
                    • Check if the server address is correct\n\
                    • Verify the server is running and accessible",
                    address
                )
            }
            Self::ActorNotFound { actor_id } => {
                format!(
                    "Actor '{}' was not found.\n\n\
                    Possible solutions:\n\
                    • Check the actor ID is correct\n\
                    • List running actors with: theater list\n\
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
            Self::NotImplemented { feature, message } => {
                format!(
                    "Feature '{}' is not implemented.\n\n\
                    {}\n\n\
                    ",
                    feature, message
                )
            }
            _ => self.to_string(),
        }
    }

    /// Check if this error suggests the user should retry
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed { .. }
                | Self::ConnectionLost
                | Self::ConnectionTimeout { .. }
                | Self::ServerError { .. }
        )
    }

    /// Get the error category for metrics/logging
    pub fn category(&self) -> &'static str {
        match self {
            Self::ConnectionFailed { .. }
            | Self::ConnectionLost
            | Self::ConnectionTimeout { .. } => "connection",
            Self::ActorNotFound { .. }
            | Self::ActorStartFailed { .. }
            | Self::ActorNotRunning { .. } => "actor",
            Self::InvalidProjectDirectory { .. }
            | Self::BuildFailed { .. }
            | Self::MissingTool { .. } => "build",
            Self::InvalidManifest { .. } | Self::ConfigError { .. } => "config",
            Self::TemplateNotFound { .. } | Self::TemplateError { .. } => "template",
            Self::FileOperationFailed { .. } | Self::PermissionDenied { .. } => "filesystem",
            Self::InvalidInput { .. } | Self::ValidationError { .. } => "validation",
            Self::ServerError { .. }
            | Self::ProtocolError { .. }
            | Self::UnexpectedResponse { .. } => "server",
            Self::EventStreamError { .. } | Self::EventFilterError { .. } => "events",
            Self::Internal(_) | Self::Serialization(_) | Self::Io(_) | Self::ParseError { .. } => {
                "internal"
            }
            Self::NetworkError { .. } => "network",
            Self::InvalidResponse { .. } => "response",
            Self::IoError { .. } => "io",
            Self::NotImplemented { .. } => "not_implemented",
        }
    }
}

/// Result type alias for CLI operations
pub type CliResult<T> = Result<T, CliError>;

/// Extension trait for converting common errors to CliError
pub trait IntoCliError<T> {
    fn into_cli_error(self) -> CliResult<T>;
    fn with_cli_context(self, context: impl FnOnce() -> CliError) -> CliResult<T>;
}

impl<T, E> IntoCliError<T> for Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn into_cli_error(self) -> CliResult<T> {
        self.map_err(|e| CliError::Internal(e.into()))
    }

    fn with_cli_context(self, context: impl FnOnce() -> CliError) -> CliResult<T> {
        self.map_err(|_| context())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_categories() {
        assert_eq!(CliError::actor_not_found("test").category(), "actor");
        assert_eq!(CliError::build_failed("failed").category(), "build");
        assert_eq!(
            CliError::template_not_found("basic", vec!["http".to_string()]).category(),
            "template"
        );
    }

    #[test]
    fn test_retryable_errors() {
        assert!(CliError::ConnectionLost.is_retryable());
        assert!(!CliError::actor_not_found("test").is_retryable());
    }

    #[test]
    fn test_user_messages() {
        let error = CliError::actor_not_found("test-actor");
        let message = error.user_message();
        assert!(message.contains("test-actor"));
        assert!(message.contains("theater list"));
    }
}
