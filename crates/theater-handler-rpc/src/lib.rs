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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, info};

/// Static-metadata → schema serialization for the `describe` verb.
mod describe;

/// Configuration for the RPC handler
/// This handler enables direct actor-to-actor function calls
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RpcHandlerConfig {
    /// Which actors may `call` this one: `"any"`, `"spawner"`, `"children"`
    /// (actors it spawned) or an actor id; `[]` = no actor, only the host.
    /// Its spawner may always call it. Calls from the host (an `ActorHandle`,
    /// not rpc) are not checked. Unset in a manifest block keeps the rule
    /// the actor would get without the block (the host's, or its spawner's),
    /// so a bare `[[handler]] type = "rpc"` widens nothing; a handler with no
    /// rule anywhere takes calls from any actor.
    #[serde(default)]
    pub callers: Option<Vec<String>>,
}

impl RpcHandlerConfig {
    /// A child's config may only narrow this (the spawner's): each caller
    /// it names must be in the spawner's list, or be its spawner. Names are
    /// compared as written ("children" means each actor's own children).
    /// Unset keeps the spawner's rule, so it always passes.
    fn check_child(&self, child: &Self) -> Result<(), String> {
        let Some(ref allowed) = self.callers else {
            return Ok(());
        };
        if allowed.iter().any(|c| c == "any") {
            return Ok(());
        }
        match child.callers {
            None => Ok(()),
            Some(ref asked) => match asked
                .iter()
                .find(|c| *c != "spawner" && !allowed.contains(c))
            {
                Some(c) => Err(format!(
                    "callers entry {c:?} is not in the spawner's {allowed:?}"
                )),
                None => Ok(()),
            },
        }
    }
}

/// An actor as an rpc callee: who spawned it and who may call it.
#[derive(Debug, Clone)]
struct Callee {
    parent: Option<TheaterId>,
    callers: Option<Vec<String>>,
}

/// Every actor with this handler, shared by all its instances in a runtime,
/// so a `call` can check the target's rule. Written when an actor's host
/// functions are registered, dropped at its shutdown.
type Callees = Arc<Mutex<HashMap<TheaterId, Callee>>>;

/// `caller` may call `target`. A target with no entry (it never registered
/// this handler) takes calls from any actor, like one with no `callers`.
fn check_call(callees: &Callees, caller: TheaterId, target: TheaterId) -> Result<(), String> {
    let callees = callees.lock().unwrap();
    let Some(callee) = callees.get(&target) else {
        return Ok(());
    };
    let Some(ref callers) = callee.callers else {
        return Ok(());
    };
    if callee.parent == Some(caller) {
        return Ok(());
    }
    let caller_parent = callees.get(&caller).and_then(|c| c.parent);
    let admitted = callers.iter().any(|c| match c.as_str() {
        "any" => true,
        "spawner" => false, // the spawner was admitted above
        "children" => caller_parent == Some(target),
        id => id.parse::<TheaterId>().ok() == Some(caller),
    });
    if admitted {
        return Ok(());
    }
    let who = if callers.is_empty() {
        "no actor (only the host)".to_string()
    } else {
        callers.join(", ")
    };
    Err(format!(
        "rpc refused: {target} accepts calls from: {who} (not {caller})"
    ))
}

use theater::actor::handle::ActorHandle;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

// Pack integration
use theater::pack_bridge::{
    pact_result_host_fn, parse_pact, InterfaceImpl, TypeHash, Value, ValueType,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Embedded rpc.pact file content
const RPC_PACT: &str = include_str!("../rpc.pact");

/// Declare the theater:simple/rpc interface from the pact file.
///
/// Functions:
/// - call(actor-id: string, function: string, params: value, options: value) -> value
/// - implements(actor-id: string, interface: string) -> value
/// - exports(actor-id: string) -> value
///
/// Note: The actual return types are wrapped results (variant with ok/err cases).
/// We declare them as `value` to match how actors declare them in pack_types!.
fn rpc_interface() -> InterfaceImpl {
    let pact = parse_pact(RPC_PACT).expect("embedded rpc.pact should be valid");
    InterfaceImpl::from_pact(&pact)
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
            Value::Option {
                value: Some(inner), ..
            } => match inner.as_ref() {
                Value::Record { fields, .. } => {
                    let mut options = CallOptions::default();
                    for (name, val) in fields {
                        if name == "timeout-ms" {
                            if let Value::Option {
                                value: Some(inner), ..
                            } = val
                            {
                                if let Value::U64(ms) = inner.as_ref() {
                                    options.timeout_ms = Some(*ms);
                                }
                            }
                        }
                    }
                    Some(options)
                }
                _ => None,
            },
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
    theater_tx: UnboundedSender<TheaterCommand>,
    config: RpcHandlerConfig,
    callees: Callees,
    actor_id: Arc<Mutex<Option<TheaterId>>>,
}

impl RpcHandler {
    /// A handler whose actors take calls from any actor.
    pub fn new(theater_tx: UnboundedSender<TheaterCommand>) -> Self {
        Self::with_config(theater_tx, RpcHandlerConfig::default())
    }

    /// A handler whose actors take calls as `config.callers` says, unless
    /// their manifest narrows it.
    pub fn with_config(
        theater_tx: UnboundedSender<TheaterCommand>,
        config: RpcHandlerConfig,
    ) -> Self {
        Self {
            theater_tx,
            config,
            callees: Arc::new(Mutex::new(HashMap::new())),
            actor_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![rpc_interface()]
    }
}

impl Handler for RpcHandler {
    fn create_instance(
        &self,
        config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        let mut config = match config {
            Some(c) => c.parse::<RpcHandlerConfig>().unwrap_or_else(|e| {
                tracing::warn!("invalid rpc handler config: {}; using default", e);
                self.config.clone()
            }),
            None => self.config.clone(),
        };
        // A block without `callers` keeps this handler's rule.
        if config.callers.is_none() {
            config.callers = self.config.callers.clone();
        }
        Box::new(RpcHandler {
            theater_tx: self.theater_tx.clone(),
            config,
            callees: self.callees.clone(), // shared across instances
            actor_id: Arc::new(Mutex::new(None)),
        })
    }

    fn check_child_config(
        &self,
        config: &theater::config::actor_manifest::HandlerConfig,
    ) -> Result<(), String> {
        let child = config
            .parse::<RpcHandlerConfig>()
            .map_err(|e| format!("invalid config: {}", e))?;
        self.config.check_child(&child)
    }

    fn setup(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("RPC handler setup");
        let callees = self.callees.clone();
        let actor_id = self.actor_id.clone();

        Box::pin(async move {
            // RPC handler doesn't need a background task
            shutdown_receiver.wait_for_shutdown().await;
            info!("RPC handler received shutdown signal");
            if let Some(id) = *actor_id.lock().unwrap() {
                callees.lock().unwrap().remove(&id);
            }
            Ok(())
        })
    }

    fn register_host_functions(
        &mut self,
        imports: &mut theater::pack_bridge::HostImports,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up RPC host functions");

        // Record this actor's rule before anything else, so it is a callee
        // with that rule even when another handler provides its imports.
        let caller = ctx
            .actor_id
            .ok_or_else(|| anyhow::anyhow!("actor_id not set in HandlerContext"))?;
        *self.actor_id.lock().unwrap() = Some(caller);
        self.callees.lock().unwrap().insert(
            caller,
            Callee {
                parent: ctx.parent_id,
                callers: self.config.callers.clone(),
            },
        );

        // Check if the interface is already satisfied
        if ctx.is_satisfied("theater:simple/rpc") {
            info!("theater:simple/rpc already satisfied, skipping");
            return Ok(());
        }

        // ----------------------------------------------------------------
        // call: Call a function on another actor
        // ----------------------------------------------------------------
        let theater_tx = self.theater_tx.clone();
        let callees = self.callees.clone();
        imports.define(
            "theater:simple/rpc",
            "call",
            pact_result_host_fn(move |input: Value| {
                let theater_tx = theater_tx.clone();
                let callees = callees.clone();

                async move {
                    // Parse input: tuple of (actor-id, function, params, options)
                    let (actor_id_str, function, params, options) = match &input {
                        Value::Tuple(items) if items.len() >= 3 => {
                            let actor_id = match &items[0] {
                                Value::String(s) => s.clone(),
                                _ => return Err::<Value, Value>(Value::String(String::from("Invalid actor-id: expected string"))),
                            };
                            let function = match &items[1] {
                                Value::String(s) => s.clone(),
                                _ => return Err::<Value, Value>(Value::String(String::from("Invalid function: expected string"))),
                            };
                            let params = items[2].clone();
                            let options = if items.len() > 3 {
                                CallOptions::from_value(&items[3])
                            } else {
                                None
                            };
                            (actor_id, function, params, options)
                        }
                        _ => return Err::<Value, Value>(Value::String(String::from("Invalid input: expected tuple of (actor-id, function, params, options)"))),
                    };

                    debug!("RPC call: actor={}, function={}", actor_id_str, function);

                    // Parse actor ID
                    let target_id = match actor_id_str.parse::<TheaterId>() {
                        Ok(id) => id,
                        Err(e) => return Err::<Value, Value>(Value::String(format!("Invalid actor ID: {}", e))),
                    };
                    check_call(&callees, caller, target_id).map_err(Value::String)?;

                    // Get actor handle from theater runtime
                    let (response_tx, response_rx) = oneshot::channel();
                    if let Err(e) = theater_tx
                        .send(TheaterCommand::GetActorHandle {
                            actor_id: target_id,
                            response_tx,
                        })
                    {
                        return Err::<Value, Value>(Value::String(format!("Failed to send to theater: {}", e)));
                    }

                    let target_handle = match response_rx.await {
                        Ok(Some(handle)) => handle,
                        Ok(None) => return Err::<Value, Value>(Value::String(format!("Actor not found: {}", actor_id_str))),
                        Err(e) => return Err::<Value, Value>(Value::String(format!("Failed to get actor handle: {}", e))),
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
                        // Return the callee's value directly. `pact_result_host_fn`
                        // already supplies the single first-class `result` transport
                        // (Ok -> result::ok(value)); the old manual
                        // `Value::Variant{type_name:"result", case_name:"ok"}` was a
                        // redundant SECOND (legacy-form) wrap that consumers had to
                        // peel through — the root cause of the "router rpc failed"
                        // double-decode. Drop it.
                        Ok(Ok(value)) => Ok(value),
                        // rpc-transport failures ride the transport's err arm
                        // (Err -> result::err(string)), kept DISTINCT from a callee
                        // err (which is nested inside the ok payload).
                        Ok(Err(e)) => Err(Value::String(format!("Call failed: {}", e))),
                        Err(_) => Err(Value::String(String::from("Call timed out"))),
                    }
                }
            }),
        );
        // ----------------------------------------------------------------
        // implements: Check if actor exports an interface
        // ----------------------------------------------------------------
        let theater_tx = self.theater_tx.clone();
        imports.define(
            "theater:simple/rpc",
            "implements",
            pact_result_host_fn(move |input: Value| {
                let theater_tx = theater_tx.clone();

                async move {
                    // Parse input: tuple of (actor-id, interface)
                    let (actor_id_str, interface_name) = match &input {
                        Value::Tuple(items) if items.len() >= 2 => {
                            let actor_id = match &items[0] {
                                Value::String(s) => s.clone(),
                                _ => {
                                    return Ok::<Value, Value>(make_error(
                                        "Invalid actor-id: expected string",
                                    ))
                                }
                            };
                            let interface = match &items[1] {
                                Value::String(s) => s.clone(),
                                _ => {
                                    return Ok::<Value, Value>(make_error(
                                        "Invalid interface: expected string",
                                    ))
                                }
                            };
                            (actor_id, interface)
                        }
                        _ => {
                            return Ok::<Value, Value>(make_error(
                                "Invalid input: expected tuple of (actor-id, interface)",
                            ))
                        }
                    };

                    debug!(
                        "RPC implements check: actor={}, interface={}",
                        actor_id_str, interface_name
                    );

                    // Parse actor ID
                    let target_id = match actor_id_str.parse::<TheaterId>() {
                        Ok(id) => id,
                        Err(e) => {
                            return Ok::<Value, Value>(make_error(&format!(
                                "Invalid actor ID: {}",
                                e
                            )))
                        }
                    };

                    // Get actor's export hashes from theater runtime
                    let (response_tx, response_rx) = oneshot::channel();
                    if let Err(e) = theater_tx.send(TheaterCommand::GetActorExportHashes {
                        actor_id: target_id,
                        response_tx,
                    }) {
                        return Ok::<Value, Value>(make_error(&format!(
                            "Failed to send to theater: {}",
                            e
                        )));
                    }

                    let export_hashes = match response_rx.await {
                        Ok(Some(hashes)) => hashes,
                        Ok(None) => {
                            return Ok::<Value, Value>(make_error(&format!(
                                "Actor not found: {}",
                                actor_id_str
                            )))
                        }
                        Err(e) => {
                            return Ok::<Value, Value>(make_error(&format!(
                                "Failed to get export hashes: {}",
                                e
                            )))
                        }
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
            }),
        );
        // ----------------------------------------------------------------
        // exports: Get list of actor's exported interfaces
        // ----------------------------------------------------------------
        let theater_tx = self.theater_tx.clone();
        imports.define(
            "theater:simple/rpc",
            "exports",
            pact_result_host_fn(move |input: Value| {
                let theater_tx = theater_tx.clone();

                async move {
                    // Parse input: actor-id string (might be wrapped in tuple)
                    let actor_id_str = match &input {
                        Value::String(s) => s.clone(),
                        Value::Tuple(items) if !items.is_empty() => match &items[0] {
                            Value::String(s) => s.clone(),
                            _ => {
                                return Ok::<Value, Value>(make_error(
                                    "Invalid actor-id: expected string",
                                ))
                            }
                        },
                        _ => {
                            return Ok::<Value, Value>(make_error(
                                "Invalid input: expected actor-id string",
                            ))
                        }
                    };

                    debug!("RPC exports query: actor={}", actor_id_str);

                    // Parse actor ID
                    let target_id = match actor_id_str.parse::<TheaterId>() {
                        Ok(id) => id,
                        Err(e) => {
                            return Ok::<Value, Value>(make_error(&format!(
                                "Invalid actor ID: {}",
                                e
                            )))
                        }
                    };

                    // Get actor's export hashes from theater runtime
                    let (response_tx, response_rx) = oneshot::channel();
                    if let Err(e) = theater_tx.send(TheaterCommand::GetActorExportHashes {
                        actor_id: target_id,
                        response_tx,
                    }) {
                        return Ok::<Value, Value>(make_error(&format!(
                            "Failed to send to theater: {}",
                            e
                        )));
                    }

                    let export_hashes = match response_rx.await {
                        Ok(Some(hashes)) => hashes,
                        Ok(None) => {
                            return Ok::<Value, Value>(make_error(&format!(
                                "Actor not found: {}",
                                actor_id_str
                            )))
                        }
                        Err(e) => {
                            return Ok::<Value, Value>(make_error(&format!(
                                "Failed to get export hashes: {}",
                                e
                            )))
                        }
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
            }),
        );
        // ----------------------------------------------------------------
        // describe: full export/import schema from the actor's STATIC module
        // metadata (no application export executed). Returns a structured
        // dynamic value — see describe.rs for the shape.
        // ----------------------------------------------------------------
        let theater_tx = self.theater_tx.clone();
        imports.define(
            "theater:simple/rpc",
            "describe",
            pact_result_host_fn(move |input: Value| {
                let theater_tx = theater_tx.clone();

                async move {
                    // NOTE: return Ok(value) / Err(string) — pact_result_host_fn
                    // wraps these into the single Pact `result`, so an error
                    // decodes guest-side as result::err (NOT a nested
                    // result::ok(result::err) that unwraps to success).
                    let actor_id_str = match &input {
                        Value::String(s) => s.clone(),
                        Value::Tuple(items) if !items.is_empty() => match &items[0] {
                            Value::String(s) => s.clone(),
                            _ => {
                                return Err(Value::String(
                                    "Invalid actor-id: expected string".to_string(),
                                ))
                            }
                        },
                        _ => {
                            return Err(Value::String(
                                "Invalid input: expected actor-id string".to_string(),
                            ))
                        }
                    };

                    debug!("RPC describe query: actor={}", actor_id_str);

                    let target_id = match actor_id_str.parse::<TheaterId>() {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Invalid actor ID: {}", e))),
                    };

                    let (response_tx, response_rx) = oneshot::channel();
                    if let Err(e) = theater_tx.send(TheaterCommand::GetActorMetadata {
                        actor_id: target_id,
                        response_tx,
                    }) {
                        return Err(Value::String(format!("Failed to send to theater: {}", e)));
                    }

                    let metadata = match response_rx.await {
                        Ok(Some(md)) => md,
                        Ok(None) => {
                            return Err(Value::String(format!("Actor not found: {}", actor_id_str)))
                        }
                        Err(e) => {
                            return Err(Value::String(format!("Failed to get metadata: {}", e)))
                        }
                    };

                    Ok(describe::describe_metadata(&metadata))
                }
            }),
        );

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
        let (tx, _rx) = mpsc::unbounded_channel();
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
        let (tx, _rx) = mpsc::unbounded_channel();
        let handler = RpcHandler::new(tx);

        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/rpc");

        // Hash should be non-zero
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }

    fn callees(entries: &[(TheaterId, Option<TheaterId>, Option<Vec<&str>>)]) -> Callees {
        let map = entries
            .iter()
            .map(|(id, parent, callers)| {
                (
                    *id,
                    Callee {
                        parent: *parent,
                        callers: callers
                            .as_ref()
                            .map(|c| c.iter().map(|s| s.to_string()).collect()),
                    },
                )
            })
            .collect();
        Arc::new(Mutex::new(map))
    }

    #[test]
    fn callers_rule_is_checked_on_the_target() {
        let [parent, open, locked, kids, child, grandchild, stranger] =
            std::array::from_fn(|_| TheaterId::generate());
        let fixed = TheaterId::generate();
        let fixed_s = fixed.to_string();
        let c = callees(&[
            (parent, None, Some(vec!["children"])),
            (open, None, None),
            (locked, Some(parent), Some(vec![])),
            (kids, None, Some(vec!["children", fixed_s.as_str()])),
            (child, Some(kids), Some(vec![])),
            (grandchild, Some(child), Some(vec![])),
            (stranger, None, None),
        ]);
        let ok = |from, to| check_call(&c, from, to).is_ok();

        // No rule, or no entry at all: any actor.
        assert!(ok(stranger, open));
        assert!(ok(stranger, TheaterId::generate()));
        // []: no actor but the spawner.
        assert!(ok(parent, locked), "the spawner may always call");
        assert!(!ok(stranger, locked));
        let e = check_call(&c, stranger, locked).unwrap_err();
        assert!(
            e.contains("accepts calls from: no actor (only the host)"),
            "{e}"
        );
        assert!(e.contains(&stranger.to_string()), "{e}");
        // "children": direct children only, and a listed id.
        assert!(ok(child, kids));
        assert!(!ok(grandchild, kids));
        assert!(ok(fixed, kids));
        assert!(!ok(stranger, kids));
        let e = check_call(&c, stranger, kids).unwrap_err();
        assert!(
            e.contains(&format!("accepts calls from: children, {fixed}")),
            "{e}"
        );
        // A child calling its parent needs the parent to name "children".
        assert!(ok(locked, parent));
        assert!(!ok(grandchild, locked));
    }

    #[test]
    fn child_rule_may_only_narrow_the_spawner_rule() {
        let rule = |c: Option<Vec<&str>>| RpcHandlerConfig {
            callers: c.map(|v| v.into_iter().map(String::from).collect()),
        };
        // An open spawner admits any child rule.
        assert!(rule(None).check_child(&rule(None)).is_ok());
        assert!(rule(Some(vec!["any"])).check_child(&rule(None)).is_ok());

        let spawner = rule(Some(vec!["children"]));
        assert!(spawner.check_child(&rule(Some(vec![]))).is_ok());
        assert!(spawner.check_child(&rule(Some(vec!["children"]))).is_ok());
        assert!(
            spawner.check_child(&rule(Some(vec!["spawner"]))).is_ok(),
            "its spawner may call it anyway"
        );
        assert!(
            spawner.check_child(&rule(None)).is_ok(),
            "unset keeps the spawner's rule"
        );
        let e = spawner.check_child(&rule(Some(vec!["any"]))).unwrap_err();
        assert!(e.contains("callers entry \"any\""), "{e}");
        let id = TheaterId::generate().to_string();
        assert!(spawner.check_child(&rule(Some(vec![id.as_str()]))).is_err());
    }

    #[test]
    fn a_block_without_callers_keeps_the_rule() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let host = RpcHandler::with_config(
            tx,
            RpcHandlerConfig {
                callers: Some(vec![]),
            },
        );
        let (id, other) = (TheaterId::generate(), TheaterId::generate());
        // A bare `[[handler]] type = "rpc"` block, as manifests enable it.
        let mut instance = host.create_instance(Some(
            &theater::config::actor_manifest::HandlerConfig::unit("rpc"),
        ));
        let mut ctx = HandlerContext::new();
        ctx.actor_id = Some(id);
        instance
            .register_host_functions(&mut theater::pack_bridge::HostImports::new(), &mut ctx)
            .unwrap();
        let e = check_call(&host.callees, other, id).unwrap_err();
        assert!(e.contains("no actor (only the host)"), "{e}");
    }

    /// Register `instance` as actor `id` (with `parent`), as the runtime does.
    fn register(instance: &mut Box<dyn Handler>, id: TheaterId, parent: Option<TheaterId>) {
        let mut ctx = HandlerContext::new();
        ctx.actor_id = Some(id);
        ctx.parent_id = parent;
        instance
            .register_host_functions(&mut theater::pack_bridge::HostImports::new(), &mut ctx)
            .unwrap();
    }

    #[test]
    fn a_bare_block_under_a_host_with_no_rule_takes_any_caller() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let host = RpcHandler::new(tx);
        let bare = theater::config::actor_manifest::HandlerConfig::unit("rpc");
        let id = TheaterId::generate();
        register(&mut host.create_instance(Some(&bare)), id, None);
        assert!(check_call(&host.callees, TheaterId::generate(), id).is_ok());
    }

    #[test]
    fn a_bare_block_under_a_locked_spawner_is_accepted_and_inherits_its_rule() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let host = RpcHandler::new(tx);
        let bare = theater::config::actor_manifest::HandlerConfig::unit("rpc");
        let (spawner_id, child_id, other) = (
            TheaterId::generate(),
            TheaterId::generate(),
            TheaterId::generate(),
        );
        // The spawner's own instance has callers = []; a child starts from it.
        let mut spawner =
            host.create_instance(Some(&theater::config::actor_manifest::HandlerConfig::new(
                "rpc",
                RpcHandlerConfig {
                    callers: Some(vec![]),
                },
            )));
        register(&mut spawner, spawner_id, None);
        spawner
            .check_child_config(&bare)
            .expect("a bare block is accepted under []");
        register(
            &mut spawner.create_instance(Some(&bare)),
            child_id,
            Some(spawner_id),
        );
        assert!(check_call(&host.callees, spawner_id, child_id).is_ok());
        let e = check_call(&host.callees, other, child_id).unwrap_err();
        assert!(
            e.contains("no actor (only the host)"),
            "the child inherits []: {e}"
        );
    }

    #[test]
    fn registering_host_functions_records_the_rule_and_parent() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let template = RpcHandler::with_config(
            tx,
            RpcHandlerConfig {
                callers: Some(vec![]),
            },
        );
        let (parent, id, other) = (
            TheaterId::generate(),
            TheaterId::generate(),
            TheaterId::generate(),
        );
        let mut instance = template.create_instance(None);
        let mut ctx = HandlerContext::new();
        ctx.actor_id = Some(id);
        ctx.parent_id = Some(parent);
        instance
            .register_host_functions(&mut theater::pack_bridge::HostImports::new(), &mut ctx)
            .unwrap();
        assert!(check_call(&template.callees, parent, id).is_ok());
        assert!(check_call(&template.callees, other, id).is_err());
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
