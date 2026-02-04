//! # Assembler Handler
//!
//! Provides WAT to WASM assembly capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to convert WebAssembly Text format (WAT) to binary WASM.

use std::future::Future;
use std::pin::Pin;

use tracing::info;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;

// Pack integration
use theater::pack_bridge::{Ctx, HostLinkerBuilder, LinkerError, Value, ValueType};

/// Handler for providing WAT to WASM assembly capabilities
#[derive(Clone, Default)]
pub struct AssemblerHandler;

impl AssemblerHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Handler for AssemblerHandler {
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
        info!("Starting assembler handler");

        Box::pin(async {
            // Assembler handler doesn't need a background task, just wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;
            info!("Assembler handler received shutdown signal");
            info!("Assembler handler shut down");
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up assembler host functions (Pack)");

        // Check if the interface is already satisfied by another handler
        if ctx.is_satisfied("wisp:assembler/runtime") {
            info!("wisp:assembler/runtime already satisfied by another handler, skipping");
            return Ok(());
        }

        builder
            .interface("wisp:assembler/runtime")?
            // wat-to-wasm: func(wat: string) -> result<list<u8>, string>
            .func_typed_result(
                "wat-to-wasm",
                |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                    let wat = match input {
                        Value::String(s) => s,
                        _ => {
                            return Err(Value::String("expected string argument".to_string()));
                        }
                    };

                    info!("[ASSEMBLER] Converting {} bytes of WAT to WASM", wat.len());

                    match wat::parse_str(&wat) {
                        Ok(wasm_bytes) => {
                            info!("[ASSEMBLER] Success: {} bytes of WASM", wasm_bytes.len());
                            let bytes: Vec<Value> =
                                wasm_bytes.into_iter().map(Value::U8).collect();
                            Ok(Value::List {
                                elem_type: ValueType::U8,
                                items: bytes,
                            })
                        }
                        Err(e) => {
                            info!("[ASSEMBLER] Error: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                },
            )?;

        ctx.mark_satisfied("wisp:assembler/runtime");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "assembler"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(vec!["wisp:assembler/runtime".to_string()])
    }

    fn exports(&self) -> Option<Vec<String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assembler_handler_creation() {
        let handler = AssemblerHandler::new();
        assert_eq!(handler.name(), "assembler");
        assert_eq!(
            handler.imports(),
            Some(vec!["wisp:assembler/runtime".to_string()])
        );
        assert_eq!(handler.exports(), None);
    }

    #[test]
    fn test_wat_to_wasm_basic() {
        // Test that the wat crate itself works
        let wat = "(module)";
        let result = wat::parse_str(wat);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wat_to_wasm_with_function() {
        let wat = r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add
                )
            )
        "#;
        let result = wat::parse_str(wat);
        assert!(result.is_ok());
        assert!(result.unwrap().len() > 0);
    }
}
