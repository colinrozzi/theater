use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentEventData {
    pub operation: String,     // "get-var", "exists", "list-vars"
    pub variable_name: String, // The variable name accessed
    pub success: bool,         // Whether the operation was allowed
    pub value_found: bool,     // Whether the variable actually existed
    pub timestamp: DateTime<Utc>,
}
