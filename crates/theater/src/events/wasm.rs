use crate::pack_bridge::{GraphValue, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, GraphValue)]
pub enum WasmEventData {
    WasmCall {
        function_name: String,
        params: Value,
    },
    WasmResult {
        function_name: String,
        state: Value,
        response: Value,
    },
    WasmError {
        function_name: String,
        message: String,
    },
    WasmComponentCreationError {
        error: String,
    },
}

pub struct WasmEvent {
    pub data: WasmEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
