use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EnvironmentEventData {
    // Environment variable access events
    #[serde(rename = "get_var")]
    GetVar {
        variable_name: String,
        success: bool,
        value_found: bool,
        timestamp: DateTime<Utc>,
    },
    
    #[serde(rename = "permission_denied")]
    PermissionDenied {
        operation: String,
        variable_name: String,
        reason: String,
    },
    
    #[serde(rename = "error")]
    Error {
        operation: String,
        message: String,
    },

    // Handler setup events
    HandlerSetupStart,
    HandlerSetupSuccess,
    HandlerSetupError {
        error: String,
        step: String,
    },
    LinkerInstanceSuccess,
    FunctionSetupStart {
        function_name: String,
    },
    FunctionSetupSuccess {
        function_name: String,
    },
}
