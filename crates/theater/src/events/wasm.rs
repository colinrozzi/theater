use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WasmEventData {
    WasmCall {
        function_name: String,
        params: Vec<u8>,
    },
    WasmResult {
        function_name: String,
        result: (Option<Vec<u8>>, Vec<u8>),
    },
    WasmError {
        function_name: String,
        message: String,
    },
}

pub struct WasmEvent {
    pub data: WasmEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
