//! # RPC Handler
//!
//! Provides direct actor-to-actor function calls in the Theater system.
//! This handler enables actors to call functions on other actors with full
//! type safety via Pack's interface hashing.
//!
//! ## Interface
//!
//! ```wit
//! interface rpc {
//!     record call-options {
//!         timeout-ms: option<u64>,
//!     }
//!
//!     call: func(actor-id: string, function: string, params: value, options: option<call-options>) -> result<value, string>;
//!     implements: func(actor-id: string, interface: string) -> result<bool, string>;
//!     exports: func(actor-id: string) -> result<list<string>, string>;
//! }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tracing::{debug, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

// Pack integration
use theater::pack_bridge::{
    AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Declare the theater:simple/rpc interface.
///
/// Functions:
/// - call(actor-id: string, function: string, params: value, options: value) -> value
/// - implements(actor-id: string, interface: string) -> value
/// - exports(actor-id: string) -> value
///
/// Note: The actual return types are wrapped results (variant with ok/err cases).
/// We declare them as `value` to match how actors declare them in pack_types!.
fn rpc_interface() -> InterfaceImpl {
    InterfaceImpl::new("theater:simple/rpc")
        // call: (string, string, value, value) -> value
        .func(
            "call",
            |_: String, _: String, _: Value, _: Value| -> Value {
                Value::Bool(false) // Dummy implementation for signature
            },
        )
        // implements: (string, string) -> value
        .func(
            "implements",
            |_: String, _: String| -> Value {
                Value::Bool(false)
            },
        )
        // exports: (string) -> value
        .func("exports", |_: String| -> Value {
            Value::Bool(false)
        })
}

// ============================================================================
// Call Options
// ============================================================================

/// Options for RPC calls
#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    /// Timeout in milliseconds
    pub timeout_ms: Option<u64>,
}

impl CallOptions {
    /// Parse call options from a Pack Value
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Option { value: Some(inner), .. } => {
                match inner.as_ref() {
                    Value::Record { fields, .. } => {
                        let mut options = CallOptions::default();
                        for (name, val) in fields {
                            if name == "timeout-ms" {
                                if let Value::Option { value: Some(inner), .. } = val {
                                    if let Value::U64(ms) = inner.as_ref() {
                                        options.timeout_ms = Some(*ms);
                                    }
                                }
                            }
                        }
                        Some(options)
                    }
                    _ => None,
                }
            }
            Value::Option { value: None, .. } => None,
            _ => None,
        }
    }
}

// ============================================================================
// Handler Implementation
// ============================================================================

/// Handler for providing RPC capabilities to actors
#[derive(Clone)]
pub struct RpcHandler {
    theater_tx: Sender<TheaterCommand>,
}

impl RpcHandler {
    pub fn new(theater_tx: Sender<TheaterCommand>) -> Self {
        Self { theater_tx }
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![rpc_interface()]
    }
}

impl Handler for RpcHandler {
    fn create_instance(
        &self,
        _config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting RPC handler");

        Box::pin(async move {
            // RPC handler doesn't need a background task
            shutdown_receiver.wait_for_shutdown().await;
            info!("RPC handler received shutdown signal");
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up RPC host functions");

        // Check if the interface is already satisfied
        if ctx.is_satisfied("theater:simple/rpc") {
            info!("theater:simple/rpc already satisfied, skipping");
            return Ok(());
        }

        let theater_tx = self.theater_tx.clone();

        builder
            .interface("theater:simple/rpc")?
            // ----------------------------------------------------------------
            // call: Call a function on another actor
            // ----------------------------------------------------------------
            .func_async_result("call", move |_async_ctx: AsyncCtx<ActorStore>, input: Value| {
                let theater_tx = theater_tx.clone();

                async move {
                    // Parse input: tuple of (actor-id, function, params, options)
                    let (actor_id_str, function, params, options) = match &input {
                        Value::Tuple(items) if items.len() >= 3 => {
                            let actor_id = match &items[0] {
                                Value::String(s) => s.clone(),
                                _ => return Ok::<Value, String>(make_error("Invalid actor-id: expected string")),
                            };
                            let function = match &items[1] {
                                Value::String(s) => s.clone(),
                                _ => return Ok::<Value, String>(make_error("Invalid function: expected string")),
                            };
                            let params = items[2].clone();
                            let options = if items.len() > 3 {
                                CallOptions::from_value(&items[3])
                            } else {
                                None
                            };
                            (actor_id, function, params, options)
                        }
                        _ => return Ok::<Value, String>(make_error("Invalid input: expected tuple of (actor-id, function, params, options)")),
                    };

                    debug!("RPC call: actor={}, function={}", actor_id_str, function);

                    // Parse actor ID
                    let target_id = match actor_id_str.parse::<TheaterId>() {
                        Ok(id) => id,
                        Err(e) => return Ok::<Value, String>(make_error(&format!("Invalid actor ID: {}", e))),
                    };

                    // Get actor handle from theater runtime
                    let (response_tx, response_rx) = oneshot::channel();
                    if let Err(e) = theater_tx
                        .send(TheaterCommand::GetActorHandle {
                            actor_id: target_id.clone(),
                            response_tx,
                        })
                        .await
                    {
                        return Ok::<Value, String>(make_error(&format!("Failed to send to theater: {}", e)));
                    }

                    let target_handle = match response_rx.await {
                        Ok(Some(handle)) => handle,
                        Ok(None) => return Ok::<Value, String>(make_error(&format!("Actor not found: {}", actor_id_str))),
                        Err(e) => return Ok::<Value, String>(make_error(&format!("Failed to get actor handle: {}", e))),
                    };

                    // Call the function on the target actor
                    let timeout = options
                        .and_then(|o| o.timeout_ms)
                        .map(Duration::from_millis)
                        .unwrap_or(Duration::from_secs(30)); // Default 30s timeout

                    let result = tokio::time::timeout(
                        timeout,
                        target_handle.call_function(function, params),
                    )
                    .await;

                    match result {
                        Ok(Ok(value)) => {
                            // Success - wrap in result::ok variant
                            Ok(Value::Variant {
                                type_name: String::from("result"),
                                case_name: String::from("ok"),
                                tag: 0,
                                payload: vec![value],
                            })
                        }
                        Ok(Err(e)) => Ok(make_error(&format!("Call failed: {}", e))),
                        Err(_) => Ok(make_error("Call timed out")),
                    }
                }
            })?
            // ----------------------------------------------------------------
            // implements: Check if actor exports an interface
            // ----------------------------------------------------------------
            .func_async_result("implements", {
                let theater_tx = self.theater_tx.clone();
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let theater_tx = theater_tx.clone();

                    async move {
                        // Parse input: tuple of (actor-id, interface)
                        let (actor_id_str, interface_name) = match &input {
                            Value::Tuple(items) if items.len() >= 2 => {
                                let actor_id = match &items[0] {
                                    Value::String(s) => s.clone(),
                                    _ => return Ok::<Value, String>(make_error("Invalid actor-id: expected string")),
                                };
                                let interface = match &items[1] {
                                    Value::String(s) => s.clone(),
                                    _ => return Ok::<Value, String>(make_error("Invalid interface: expected string")),
                                };
                                (actor_id, interface)
                            }
                            _ => return Ok::<Value, String>(make_error("Invalid input: expected tuple of (actor-id, interface)")),
                        };

                        debug!("RPC implements check: actor={}, interface={}", actor_id_str, interface_name);

                        // Parse actor ID
                        let target_id = match actor_id_str.parse::<TheaterId>() {
                            Ok(id) => id,
                            Err(e) => return Ok::<Value, String>(make_error(&format!("Invalid actor ID: {}", e))),
                        };

                        // Get actor's export hashes from theater runtime
                        let (response_tx, response_rx) = oneshot::channel();
                        if let Err(e) = theater_tx
                            .send(TheaterCommand::GetActorExportHashes {
                                actor_id: target_id.clone(),
                                response_tx,
                            })
                            .await
                        {
                            return Ok::<Value, String>(make_error(&format!("Failed to send to theater: {}", e)));
                        }

                        let export_hashes = match response_rx.await {
                            Ok(Some(hashes)) => hashes,
                            Ok(None) => return Ok::<Value, String>(make_error(&format!("Actor not found: {}", actor_id_str))),
                            Err(e) => return Ok::<Value, String>(make_error(&format!("Failed to get export hashes: {}", e))),
                        };

                        // Check if interface is in exports
                        let implements = export_hashes.iter().any(|h| h.name == interface_name);

                        Ok(Value::Variant {
                            type_name: String::from("result"),
                            case_name: String::from("ok"),
                            tag: 0,
                            payload: vec![Value::Bool(implements)],
                        })
                    }
                }
            })?
            // ----------------------------------------------------------------
            // exports: Get list of actor's exported interfaces
            // ----------------------------------------------------------------
            .func_async_result("exports", {
                let theater_tx = self.theater_tx.clone();
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let theater_tx = theater_tx.clone();

                    async move {
                        // Parse input: actor-id string (might be wrapped in tuple)
                        let actor_id_str = match &input {
                            Value::String(s) => s.clone(),
                            Value::Tuple(items) if !items.is_empty() => {
                                match &items[0] {
                                    Value::String(s) => s.clone(),
                                    _ => return Ok::<Value, String>(make_error("Invalid actor-id: expected string")),
                                }
                            }
                            _ => return Ok::<Value, String>(make_error("Invalid input: expected actor-id string")),
                        };

                        debug!("RPC exports query: actor={}", actor_id_str);

                        // Parse actor ID
                        let target_id = match actor_id_str.parse::<TheaterId>() {
                            Ok(id) => id,
                            Err(e) => return Ok::<Value, String>(make_error(&format!("Invalid actor ID: {}", e))),
                        };

                        // Get actor's export hashes from theater runtime
                        let (response_tx, response_rx) = oneshot::channel();
                        if let Err(e) = theater_tx
                            .send(TheaterCommand::GetActorExportHashes {
                                actor_id: target_id.clone(),
                                response_tx,
                            })
                            .await
                        {
                            return Ok::<Value, String>(make_error(&format!("Failed to send to theater: {}", e)));
                        }

                        let export_hashes = match response_rx.await {
                            Ok(Some(hashes)) => hashes,
                            Ok(None) => return Ok::<Value, String>(make_error(&format!("Actor not found: {}", actor_id_str))),
                            Err(e) => return Ok::<Value, String>(make_error(&format!("Failed to get export hashes: {}", e))),
                        };

                        // Convert to list of interface names
                        let interface_names: Vec<Value> = export_hashes
                            .iter()
                            .map(|h| Value::String(h.name.clone()))
                            .collect();

                        Ok(Value::Variant {
                            type_name: String::from("result"),
                            case_name: String::from("ok"),
                            tag: 0,
                            payload: vec![Value::List {
                                elem_type: ValueType::String,
                                items: interface_names,
                            }],
                        })
                    }
                }
            })?;

        ctx.mark_satisfied("theater:simple/rpc");
        info!("RPC host functions set up successfully");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "rpc"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(
            self.interfaces()
                .iter()
                .map(|i| i.name().to_string())
                .collect(),
        )
    }

    fn exports(&self) -> Option<Vec<String>> {
        None // RPC handler doesn't expect any exports from the actor
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![rpc_interface()]
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Create an error result value
fn make_error(msg: &str) -> Value {
    Value::Variant {
        type_name: String::from("result"),
        case_name: String::from("err"),
        tag: 1,
        payload: vec![Value::String(msg.to_string())],
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_rpc_handler_creation() {
        let (tx, _rx) = mpsc::channel(100);
        let handler = RpcHandler::new(tx);
        assert_eq!(handler.name(), "rpc");
    }

    #[test]
    fn test_rpc_interface_hash_determinism() {
        let interface1 = rpc_interface();
        let interface2 = rpc_interface();
        assert_eq!(interface1.hash(), interface2.hash());
    }

    #[test]
    fn test_rpc_handler_interface_hashes() {
        let (tx, _rx) = mpsc::channel(100);
        let handler = RpcHandler::new(tx);

        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/rpc");

        // Hash should be non-zero
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_call_options_parsing() {
        // Test None case
        let none_value = Value::Option {
            inner_type: ValueType::Bool,
            value: None,
        };
        assert!(CallOptions::from_value(&none_value).is_none());

        // Test Some case with timeout
        let some_value = Value::Option {
            inner_type: ValueType::Bool,
            value: Some(Box::new(Value::Record {
                type_name: String::from("call-options"),
                fields: vec![(
                    String::from("timeout-ms"),
                    Value::Option {
                        inner_type: ValueType::U64,
                        value: Some(Box::new(Value::U64(5000))),
                    },
                )],
            })),
        };
        let options = CallOptions::from_value(&some_value).unwrap();
        assert_eq!(options.timeout_ms, Some(5000));
    }
}
