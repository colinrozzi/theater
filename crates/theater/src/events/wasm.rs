use crate::pack_bridge::{ConversionError, IntoValue, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl IntoValue for WasmEventData {
    fn into_value(self) -> Value {
        match self {
            WasmEventData::WasmCall {
                function_name,
                params,
            } => Value::Variant {
                type_name: String::from("wasm-event-data"),
                case_name: String::from("wasm-call"),
                tag: 0,
                payload: vec![Value::Record {
                    type_name: String::from("wasm-call"),
                    fields: vec![
                        ("function-name".into(), Value::String(function_name)),
                        ("params".into(), params),
                    ],
                }],
            },
            WasmEventData::WasmResult {
                function_name,
                state,
                response,
            } => Value::Variant {
                type_name: String::from("wasm-event-data"),
                case_name: String::from("wasm-result"),
                tag: 1,
                payload: vec![Value::Record {
                    type_name: String::from("wasm-result"),
                    fields: vec![
                        ("function-name".into(), Value::String(function_name)),
                        ("state".into(), state),
                        ("response".into(), response),
                    ],
                }],
            },
            WasmEventData::WasmError {
                function_name,
                message,
            } => Value::Variant {
                type_name: String::from("wasm-event-data"),
                case_name: String::from("wasm-error"),
                tag: 2,
                payload: vec![Value::Record {
                    type_name: String::from("wasm-error"),
                    fields: vec![
                        ("function-name".into(), Value::String(function_name)),
                        ("message".into(), Value::String(message)),
                    ],
                }],
            },
            WasmEventData::WasmComponentCreationError { error } => Value::Variant {
                type_name: String::from("wasm-event-data"),
                case_name: String::from("wasm-component-creation-error"),
                tag: 3,
                payload: vec![Value::Record {
                    type_name: String::from("wasm-component-creation-error"),
                    fields: vec![("error".into(), Value::String(error))],
                }],
            },
        }
    }
}

impl TryFrom<Value> for WasmEventData {
    type Error = ConversionError;

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Variant {
                case_name, payload, ..
            } => {
                let record = payload
                    .into_iter()
                    .next()
                    .ok_or_else(|| ConversionError::MissingField("payload".into()))?;
                let fields = match record {
                    Value::Record { fields, .. } => fields,
                    other => return Err(ConversionError::ExpectedRecord(format!("{:?}", other))),
                };

                match case_name.as_str() {
                    "wasm-call" => {
                        let mut function_name = String::new();
                        let mut params = Value::Tuple(vec![]);
                        for (name, val) in fields {
                            match name.as_str() {
                                "function-name" => function_name = String::try_from(val)?,
                                "params" => params = val,
                                _ => {}
                            }
                        }
                        Ok(WasmEventData::WasmCall {
                            function_name,
                            params,
                        })
                    }
                    "wasm-result" => {
                        let mut function_name = String::new();
                        let mut state = Value::Tuple(vec![]);
                        let mut response = Value::Tuple(vec![]);
                        for (name, val) in fields {
                            match name.as_str() {
                                "function-name" => function_name = String::try_from(val)?,
                                "state" => state = val,
                                "response" => response = val,
                                _ => {}
                            }
                        }
                        Ok(WasmEventData::WasmResult {
                            function_name,
                            state,
                            response,
                        })
                    }
                    "wasm-error" => {
                        let mut function_name = String::new();
                        let mut message = String::new();
                        for (name, val) in fields {
                            match name.as_str() {
                                "function-name" => function_name = String::try_from(val)?,
                                "message" => message = String::try_from(val)?,
                                _ => {}
                            }
                        }
                        Ok(WasmEventData::WasmError {
                            function_name,
                            message,
                        })
                    }
                    "wasm-component-creation-error" => {
                        let mut error = String::new();
                        for (name, val) in fields {
                            if name == "error" {
                                error = String::try_from(val)?;
                            }
                        }
                        Ok(WasmEventData::WasmComponentCreationError { error })
                    }
                    other => Err(ConversionError::ExpectedVariant(format!(
                        "unknown wasm-event-data case: {other}"
                    ))),
                }
            }
            other => Err(ConversionError::ExpectedVariant(format!("{:?}", other))),
        }
    }
}
