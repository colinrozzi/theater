//! # Runtime Handler
//!
//! Provides runtime information and control capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to log messages, get state information, and request shutdown.

use std::future::Future;
use std::pin::Pin;
use tracing::info;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::config::permissions::RuntimePermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use tokio::sync::mpsc::Sender;

// Pack integration
use theater::pack_bridge::{
    AsyncCtx, Ctx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Declare the theater:simple/runtime interface.
///
/// Functions:
/// - log(msg: string) -> ()
/// - get-chain() -> chain (approximated as Vec<u8> for hashing)
/// - shutdown(data: option<list<u8>>) -> result<(), string>
fn runtime_interface() -> InterfaceImpl {
    InterfaceImpl::new("theater:simple/runtime")
        .func("log", |_: String| {})
        // get-chain returns a complex record type, approximate with Vec<u8>
        .func("get-chain", || -> Vec<u8> { vec![] })
        .func("shutdown", |_: Option<Vec<u8>>| -> Result<(), String> { Ok(()) })
}

/// Handler for providing runtime information and control to WebAssembly actors
#[derive(Clone)]
pub struct RuntimeHandler {
    #[allow(dead_code)]
    config: RuntimeHostConfig,
    theater_tx: Sender<TheaterCommand>,
    #[allow(dead_code)]
    permissions: Option<RuntimePermissions>,
}

impl RuntimeHandler {
    pub fn new(
        config: RuntimeHostConfig,
        theater_tx: Sender<TheaterCommand>,
        permissions: Option<RuntimePermissions>,
    ) -> Self {
        Self {
            config,
            theater_tx,
            permissions,
        }
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![runtime_interface()]
    }
}

impl Handler for RuntimeHandler {
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting runtime handler");

        Box::pin(async {
            // Runtime handler doesn't need a background task, but we should wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;
            info!("Runtime handler received shutdown signal");
            info!("Runtime handler shut down");
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up runtime host functions (Pack)");

        // Check if the interface is already satisfied by another handler
        if ctx.is_satisfied("theater:simple/runtime") {
            info!("theater:simple/runtime already satisfied by another handler, skipping");
            return Ok(());
        }

        let theater_tx = self.theater_tx.clone();

        builder
            .interface("theater:simple/runtime")?
            // Log function: log(msg: string)
            .func_typed("log", |ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                // Extract string from Value
                let msg = match input {
                    Value::String(s) => s,
                    _ => format!("{:?}", input),
                };

                let store = ctx.data();
                let id = store.id.clone();

                info!("[ACTOR] [{}] {}", id, msg);

                // Return unit (empty tuple)
                Value::Tuple(vec![])
            })?
            // Get chain function: get-chain() -> chain
            .func_typed(
                "get-chain",
                |ctx: &mut Ctx<'_, ActorStore>, _input: Value| {
                    let store = ctx.data();
                    let events = store.get_all_events();

                    // Convert to WIT format: list<meta-event>
                    use theater::ValueType;
                    // meta-event = { hash: u64, event: event }
                    // event = { event-type: string, parent: option<u64>, data: list<u8> }
                    let chain_events: Vec<Value> = events
                        .iter()
                        .enumerate()
                        .map(|(i, e)| {
                            let hash = Value::U64(i as u64);
                            let parent = if i > 0 {
                                Value::Option {
                                    inner_type: ValueType::U64,
                                    value: Some(Box::new(Value::U64((i - 1) as u64))),
                                }
                            } else {
                                Value::Option {
                                    inner_type: ValueType::U64,
                                    value: None,
                                }
                            };
                            let event_type = Value::String(e.event_type.clone());
                            let data = Value::List {
                                elem_type: ValueType::U8,
                                items: e.data.iter().map(|b| Value::U8(*b)).collect(),
                            };

                            // meta-event record: (hash, (event-type, parent, data))
                            Value::Tuple(vec![
                                hash,
                                Value::Tuple(vec![event_type, parent, data]),
                            ])
                        })
                        .collect();

                    // Return as chain record: (events,)
                    Value::Tuple(vec![Value::List {
                        elem_type: ValueType::Tuple(vec![ValueType::U64, ValueType::Tuple(vec![ValueType::String, ValueType::Option(Box::new(ValueType::U64)), ValueType::List(Box::new(ValueType::U8))])]),
                        items: chain_events,
                    }])
                },
            )?
            // Shutdown function: shutdown(data: option<list<u8>>) -> result<(), string>
            .func_async_result(
                "shutdown",
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let theater_tx = theater_tx.clone();

                    async move {
                        // Parse input: option<list<u8>>
                        let data: Option<Vec<u8>> = match input {
                            Value::Option { value: Some(inner), .. } => match *inner {
                                Value::List { items, .. } => {
                                    let result: Result<Vec<u8>, _> = items
                                        .into_iter()
                                        .map(|v| match v {
                                            Value::U8(b) => Ok(b),
                                            _ => Err("expected u8"),
                                        })
                                        .collect();
                                    result.ok()
                                }
                                _ => None,
                            },
                            Value::Option { value: None, .. } => None,
                            _ => None,
                        };

                        let store = ctx.data();
                        let actor_id = store.id.clone();

                        info!("[ACTOR] [{}] Shutdown requested: {:?}", actor_id, data);

                        // Send shutdown command
                        match theater_tx
                            .send(TheaterCommand::ShuttingDown {
                                actor_id,
                                data,
                            })
                            .await
                        {
                            Ok(_) => Ok(Value::Tuple(vec![])),
                            Err(e) => Err(Value::String(e.to_string())),
                        }
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/runtime");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "runtime"
    }

    fn imports(&self) -> Option<Vec<String>> {
        let mut imports: Vec<String> = self.interfaces().iter().map(|i| i.name().to_string()).collect();
        // Add additional interface dependencies
        imports.push("theater:simple/types".to_string());
        Some(imports)
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/actor".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<theater::pack_bridge::InterfaceImpl> {
        vec![runtime_interface()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theater::config::actor_manifest::RuntimeHostConfig;
    use theater::pack_bridge::{
        Arena, Function, Param, Type,
        decode_metadata_with_hashes, encode_metadata_with_hashes,
    };
    use tokio::sync::mpsc;

    #[test]
    fn test_runtime_handler_creation() {
        let config = RuntimeHostConfig {};
        let (tx, _rx) = mpsc::channel(100);

        let handler = RuntimeHandler::new(config, tx, None);
        assert_eq!(handler.name(), "runtime");

        let imports = handler.imports().unwrap();
        assert!(imports.contains(&"theater:simple/runtime".to_string()));
        assert!(imports.contains(&"theater:simple/types".to_string()));

        assert_eq!(handler.exports(), Some(vec!["theater:simple/actor".to_string()]));
    }

    #[test]
    fn test_runtime_interface_hash_determinism() {
        // Creating the interface twice should produce the same hash
        let interface1 = runtime_interface();
        let interface2 = runtime_interface();
        assert_eq!(interface1.hash(), interface2.hash());
    }

    #[test]
    fn test_runtime_handler_interface_hashes() {
        let config = RuntimeHostConfig {};
        let (tx, _rx) = mpsc::channel(100);
        let handler = RuntimeHandler::new(config, tx, None);

        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/runtime");

        // Hash should be non-zero
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_hash_matching_between_actor_and_handler() {
        // Build an Arena representing an actor that imports theater:simple/runtime
        // with the same function signatures as RuntimeHandler provides
        let mut package = Arena::new("package");

        // Build imports section
        let mut imports_section = Arena::new("imports");
        let mut runtime_interface = Arena::new("theater:simple/runtime");

        // Add functions matching the runtime interface definition
        runtime_interface.add_function(Function::with_signature(
            "log",
            vec![Param::new("msg", Type::String)],
            vec![], // returns ()
        ));
        runtime_interface.add_function(Function::with_signature(
            "get-chain",
            vec![],
            vec![Type::List(Box::new(Type::U8))], // returns Vec<u8>
        ));
        runtime_interface.add_function(Function::with_signature(
            "shutdown",
            vec![Param::new("data", Type::Option(Box::new(Type::List(Box::new(Type::U8)))))],
            vec![Type::Result {
                ok: Box::new(Type::Unit),
                err: Box::new(Type::String),
            }],
        ));

        imports_section.add_child(runtime_interface);
        package.add_child(imports_section);

        // Add empty exports section
        let exports_section = Arena::new("exports");
        package.add_child(exports_section);

        // Encode metadata with hashes
        let encoded = encode_metadata_with_hashes(&package)
            .expect("should encode metadata with hashes");

        // Decode and get import hashes
        let decoded = decode_metadata_with_hashes(&encoded)
            .expect("should decode metadata with hashes");

        // The decoded import hashes should include theater:simple/runtime
        assert!(!decoded.import_hashes.is_empty(), "should have import hashes");

        let actor_runtime_hash = decoded.import_hashes
            .iter()
            .find(|h| h.name == "theater:simple/runtime")
            .expect("should have theater:simple/runtime import hash");

        // Get the handler's interface hash
        let config = RuntimeHostConfig {};
        let (tx, _rx) = mpsc::channel(100);
        let handler = RuntimeHandler::new(config, tx, None);
        let handler_hashes = handler.interface_hashes();

        let handler_runtime_hash = handler_hashes
            .iter()
            .find(|(name, _)| name == "theater:simple/runtime")
            .expect("handler should provide theater:simple/runtime");

        // The hashes should match!
        assert_eq!(
            actor_runtime_hash.hash, handler_runtime_hash.1,
            "Actor's import hash should match handler's interface hash"
        );
    }

    #[test]
    fn test_hash_mismatch_detection() {
        // Build an Arena with a DIFFERENT function signature
        // This should produce a different hash, demonstrating mismatch detection
        let mut package = Arena::new("package");

        let mut imports_section = Arena::new("imports");
        let mut runtime_interface = Arena::new("theater:simple/runtime");

        // Add a function with WRONG signature (wrong param type)
        runtime_interface.add_function(Function::with_signature(
            "log",
            vec![Param::new("msg", Type::S32)], // WRONG: should be String
            vec![],
        ));

        imports_section.add_child(runtime_interface);
        package.add_child(imports_section);
        package.add_child(Arena::new("exports"));

        // Encode and decode
        let encoded = encode_metadata_with_hashes(&package).expect("encode");
        let decoded = decode_metadata_with_hashes(&encoded).expect("decode");

        let actor_hash = decoded.import_hashes
            .iter()
            .find(|h| h.name == "theater:simple/runtime")
            .expect("should have import hash");

        // Get handler hash
        let config = RuntimeHostConfig {};
        let (tx, _rx) = mpsc::channel(100);
        let handler = RuntimeHandler::new(config, tx, None);
        let handler_hash = &handler.interface_hashes()[0].1;

        // Hashes should NOT match due to different function signature
        assert_ne!(
            actor_hash.hash, *handler_hash,
            "Mismatched signatures should produce different hashes"
        );
    }
}
