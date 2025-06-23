//! # WebAssembly Integration for Theater
//!
//! The `wasm` module provides the foundation for Theater's WebAssembly component integration,
//! enabling secure execution of actor code in isolated sandboxes. This module handles the
//! loading, instantiation, and execution of WebAssembly components, as well as type-safe
//! function calls between the host runtime and WebAssembly actors.
//!
//! ## Core Features
//!
//! * **Component Loading**: Loading and parsing WebAssembly component binaries
//! * **Instance Management**: Creating and managing WebAssembly component instances
//! * **Type-Safe Function Calls**: Safely calling functions across the WebAssembly boundary
//! * **Error Handling**: Comprehensive error reporting for WebAssembly operations
//! * **Memory Statistics**: Tracking memory usage of WebAssembly components
//!
//! ## Architecture
//!
//! The module is built around these key components:
//!
//! * `ActorComponent`: Represents a loaded WebAssembly component ready for instantiation
//! * `ActorInstance`: An instantiated WebAssembly component with registered functions
//! * `TypedFunction` trait: Provides a unified interface for calling WebAssembly functions
//! * Various implementations for different function signatures (with/without params, with/without results)
//!
//! ## Security
//!
//! This module implements critical security boundaries for Theater. WebAssembly provides
//! memory isolation and capability-based security, ensuring that actors can only access
//! resources explicitly granted to them. The module carefully validates all interactions
//! between host and WebAssembly to prevent security vulnerabilities.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use wasmtime::component::TypedFunc;
use wasmtime::component::{
    Component, ComponentExportIndex, ComponentNamedList, ComponentType, Instance, Lift, Linker,
    Lower,
};
use wasmtime::{Engine, Store};

use crate::actor::store::ActorStore;
// use crate::config::ManifestConfig;
use crate::id::TheaterId;
use crate::store;
use crate::utils::resolve_reference;
use tracing::{debug, error, info};
use wasmtime::component::types::ComponentItem;

pub type Json = Vec<u8>;

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct Event {
    #[component(name = "event-type")]
    pub event_type: String,
    pub parent: Option<u64>,
    pub data: Json,
}

/// # WebAssembly Error Types
///
/// `WasmError` represents the various errors that can occur during WebAssembly operations
/// in the Theater system. These errors provide context about what operation failed and why.
///
/// ## Purpose
///
/// This enum centralizes error handling for WebAssembly-related operations, providing
/// detailed context to help diagnose issues during component loading, instantiation,
/// and function calls. Each variant includes context about where the error occurred.
///
/// ## Example
///
/// ```rust,ignore
/// use theater::WasmError;
/// use anyhow::Result;
///
/// fn handle_wasm_operation() -> Result<(), WasmError> {
///     // Simulate a function call that might fail
///     let call_failed = true; // Example condition
///     if call_failed {
///         return Err(WasmError::WasmError {
///             context: "function_call",
///             message: "Failed to call 'greet' function".to_string(),
///         });
///     }
///     Ok(())
/// }
/// ```
///
/// ## Implementation Notes
///
/// These errors are typically converted to `anyhow::Error` before being propagated
/// to the caller, but the contextual information is preserved.
#[derive(Error, Debug)]
pub enum WasmError {
    #[error("Failed to load manifest: {0}")]
    ManifestError(String),

    #[error("WASM error: {context} - {message}")]
    WasmError {
        context: &'static str,
        message: String,
    },

    #[error("Function types were incorrect for {func_name} call \n Expected params: {expected_params} \n Expected result: {expected_result} \n Error: {err}")]
    GetFuncTypedError {
        context: &'static str,
        func_name: String,
        expected_params: String,
        expected_result: String,
        err: wasmtime::Error,
    },
}

/// # WebAssembly Memory Statistics
///
/// `MemoryStats` collects memory usage metrics for WebAssembly actors to track resource
/// consumption and detect potential memory leaks or excessive usage.
///
/// ## Purpose
///
/// This struct provides visibility into the memory consumption of individual actors,
/// allowing the runtime to implement resource limits, detect abnormal usage patterns,
/// and provide metrics for monitoring systems.
///
/// ## Example
///
/// ```rust,ignore
/// use theater::MemoryStats;
///
/// // Example of analyzing memory usage
/// fn analyze_memory(stats: &MemoryStats) {
///     println!("Actor state size: {} bytes", stats.state_size);
///     println!("Total exports table size: {} bytes", stats.exports_table_size);
///     println!("Event chain size: {} events", stats.num_chain_events);
/// }
/// ```
///
/// ## Security
///
/// Memory statistics are crucial for preventing denial-of-service attacks where
/// a malicious actor might attempt to consume excessive resources. These metrics
/// can be used to enforce memory limits and prevent resource exhaustion.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryStats {
    pub state_size: usize,
    pub exports_table_size: usize,
    pub store_size: usize,
    pub num_exports: usize,
    pub num_chain_events: usize,
}

/// # Actor WebAssembly Component
///
/// `ActorComponent` represents a loaded WebAssembly component in memory, ready to be
/// instantiated. It contains the raw component bytes, an engine for execution, and
/// metadata about available exports.
///
/// ## Purpose
///
/// This struct is the first stage in the lifecycle of a WebAssembly actor. It holds
/// the parsed component and prepares it for instantiation. The component is loaded
/// but not yet instantiated, meaning it hasn't allocated runtime resources for execution.
///
/// ## Example
///
/// ```rust,ignore
/// // ActorComponent is an internal type
/// use theater::ManifestConfig;
/// use theater::ActorStore;
/// use anyhow::Result;
///
/// // Example of loading a component (pseudo-code for internal API)
/// // async fn load_component(config: &ManifestConfig) -> Result<ActorComponent> {
/// //     let component = ActorComponent::new(config, actor_store).await?;
/// //     Ok(component)
/// // }
///     // let instance = component.instantiate().await?;
///     
///     Ok(component)
/// }
/// ```
///
/// ## Safety
///
/// This struct handles raw WebAssembly binary data and performs validation as part of
/// the component loading process. The validation ensures the component adheres to the
/// WebAssembly component model specification and can be safely instantiated.
///
/// ## Security
///
/// During component loading, the system verifies that the WebAssembly binary is valid
/// and doesn't contain prohibited instructions or invalid memory accesses. This
/// validation happens before any code is executed, providing a first layer of security.
///
/// ## Implementation Notes
///
/// Component loading uses the Wasmtime engine's component model support and is an
/// asynchronous operation because it may involve fetching component bytes from remote
/// storage or performing digest verification.
pub struct ActorComponent {
    pub name: String,
    pub component: Component,
    pub actor_store: ActorStore,
    pub linker: Linker<ActorStore>,
    pub engine: Engine,
    pub exports: HashMap<String, ComponentExportIndex>,
}

impl ActorComponent {
    /// Creates a new `ActorComponent` from a manifest configuration and actor store.
    ///
    /// ## Purpose
    ///
    /// This method loads a WebAssembly component from the path specified in the manifest
    /// configuration, initializes the Wasmtime engine, and prepares the component for
    /// instantiation. It handles resolving references to component binaries, which may
    /// be local files or content-addressed store references.
    ///
    /// ## Parameters
    ///
    /// * `config` - The manifest configuration containing the component path and metadata
    /// * `actor_store` - The actor store that will be associated with this component
    ///
    /// ## Returns
    ///
    /// * `Ok(ActorComponent)` - A loaded component ready for instantiation
    /// * `Err(anyhow::Error)` - If the component cannot be loaded or is invalid
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // ActorComponent is an internal type
    /// use theater::ManifestConfig;
    /// use theater::ActorStore;
    /// use anyhow::Result;
    ///
    /// async fn example() -> Result<()> {
    ///     let config = ManifestConfig::from_file("path/to/manifest.toml")?;
    ///     // Create an actor store with required parameters
///     // let actor_store = ActorStore::new(actor_id, theater_tx, actor_handle);
    ///     
    ///     let component = ActorComponent::new(&config, actor_store).await?;
    ///     println!("Loaded component: {}", component.name);
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## Security
    ///
    /// This method performs validation on the WebAssembly binary to ensure it adheres to
    /// the component model specification and doesn't contain prohibited instructions.
    pub async fn new(
        name: String,
        component_path: String,
        actor_store: ActorStore,
        engine: Engine,
    ) -> Result<Self> {
        // Load WASM component
        //        let engine = Engine::new(wasmtime::Config::new().async_support(true))?;
        info!("Loading WASM component from: {}", component_path);
        let _wasm_bytes = resolve_reference(&component_path).await?;
        let wasm_bytes = Self::get_wasm_component_bytes(component_path)
            .await
            .map_err(|e| {
                error!("Failed to load WASM component: {}", e);
                WasmError::WasmError {
                    context: "loading component",
                    message: e.to_string(),
                }
            })?;

        let component = Component::new(&engine, &wasm_bytes)?;
        let linker = Linker::new(&engine);

        Ok(ActorComponent {
            name,
            component,
            actor_store,
            linker,
            engine,
            exports: HashMap::new(),
        })
    }

    /// Retrieves the raw WebAssembly component bytes from the specified path.
    pub async fn get_wasm_component_bytes(component_path: String) -> Result<Vec<u8>, WasmError> {
        // IF the component path starts with https, check if we have it cached.

        let is_https =
            component_path.starts_with("https://") || component_path.starts_with("http://");

        if is_https {
            // check if we have stored the component bytes in the actor store
            let component_label = store::Label::new(component_path.clone());
            let component_store = store::ContentStore::new_named_store("wasm_component");

            let component_exists = component_store
                .label_exists(component_label.clone())
                .await
                .map_err(|e| {
                    error!("Failed to check component existence: {}", e);
                    WasmError::WasmError {
                        context: "checking component existence",
                        message: e.to_string(),
                    }
                })?;

            if component_exists {
                info!(
                    "[CACHE HIT] Component bytes found in store for label: {}",
                    component_label
                );
                let bytes = component_store
                    .get_content_by_label(&component_label)
                    .await
                    .map_err(|e| {
                        error!("Failed to get component bytes: {}", e);
                        WasmError::WasmError {
                            context: "getting component bytes",
                            message: e.to_string(),
                        }
                    })?;
                Ok(bytes.expect("Component bytes should exist in store"))
            } else {
                info!(
                    "[CACHE MISS] Component bytes not found in store, loading from path: {}",
                    component_path
                );
                let bytes =
                    resolve_reference(&component_path)
                        .await
                        .map_err(|e| WasmError::WasmError {
                            context: "resolving component reference",
                            message: e.to_string(),
                        })?;
                info!("Component bytes loaded from path: {}", component_path);
                // Store the component bytes in the content store for future use
                let bytes_ref = component_store.store(bytes.clone()).await.map_err(|e| {
                    WasmError::WasmError {
                        context: "storing component bytes",
                        message: e.to_string(),
                    }
                })?;

                // Label the stored bytes for future retrieval
                component_store
                    .label(&component_label, &bytes_ref)
                    .await
                    .map_err(|e| WasmError::WasmError {
                        context: "labeling component bytes",
                        message: e.to_string(),
                    })?;
                Ok(bytes)
            }
        } else {
            resolve_reference(&component_path)
                .await
                .map_err(|e| WasmError::WasmError {
                    context: "resolving component reference",
                    message: e.to_string(),
                })
        }
    }

    /// Finds a function export in the WebAssembly component by interface and export name.
    ///
    /// ## Purpose
    ///
    /// This method searches for a specific function export within an interface of the
    /// WebAssembly component. It validates that the export exists and is a function,
    /// and returns the export index that can be used for function calls.
    ///
    /// ## Parameters
    ///
    /// * `interface_name` - The name of the interface containing the function
    /// * `export_name` - The name of the function export to find
    ///
    /// ## Returns
    ///
    /// * `Ok(ComponentExportIndex)` - The index of the found function export
    /// * `Err(WasmError)` - If the interface or function doesn't exist, or if the export is not a function
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// # // ActorComponent and WasmError are internal types
    /// # use wasmtime::component::ComponentExportIndex;
    ///
    /// fn get_function_index(component: &mut ActorComponent) -> Result<ComponentExportIndex, WasmError> {
    ///     // Find the 'greet' function in the 'example:greeter/hello' interface
    ///     component.find_function_export("example:greeter/hello", "greet")
    /// }
    /// ```
    ///
    /// ## Security
    ///
    /// This method performs validation to ensure that the requested export is a function
    /// and not another type of export (like a module or global), which helps prevent
    /// type confusion vulnerabilities.
    ///
    /// ## Implementation Notes
    ///
    /// The method logs detailed information about the found function, including its
    /// parameter and result types, which can be helpful for debugging.
    pub fn find_function_export(
        &mut self,
        interface_name: &str,
        export_name: &str,
    ) -> Result<ComponentExportIndex, WasmError> {
        info!(
            "Finding export: {} from interface: {}",
            export_name, interface_name
        );
        let (_interface_component_item, interface_component_export_index) =
            match self.component.export_index(None, interface_name) {
                Some(export) => export,
                None => {
                    error!(
                        "Interface '{}' not found in component exports",
                        interface_name
                    );
                    return Err(WasmError::WasmError {
                        context: "find_function_export",
                        message: format!(
                            "Interface '{}' not found in component exports",
                            interface_name
                        ),
                    });
                }
            };
        info!("Found interface export: {}", interface_name);

        let (func_component_item, func_component_export_index) = match self
            .component
            .export_index(Some(&interface_component_export_index), export_name)
        {
            Some(export) => export,
            None => {
                error!(
                    "Function '{}' not found in interface '{}'",
                    export_name, interface_name
                );
                return Err(WasmError::WasmError {
                    context: "find_function_export",
                    message: format!(
                        "Function '{}' not found in interface '{}'",
                        export_name, interface_name
                    ),
                });
            }
        };
        match func_component_item {
            ComponentItem::ComponentFunc(component_func) => {
                info!("Found export: {}", export_name);
                let params = component_func.params();
                for param in params {
                    info!("Param: {:?}", param);
                }
                let results = component_func.results();
                for result in results {
                    info!("Result: {:?}", result);
                }

                Ok(func_component_export_index)
            }
            _ => {
                error!(
                    "Export {} is not a function, it is a {:?}",
                    export_name, func_component_item
                );

                Err(WasmError::WasmError {
                    context: "export type",
                    message: format!(
                        "Export {} is not a function, it is a {:?}",
                        export_name, func_component_item
                    ),
                })
            }
        }
    }

    /// Instantiates the WebAssembly component, creating a runnable instance.
    ///
    /// ## Purpose
    ///
    /// This asynchronous method takes a loaded component and creates an actual instance
    /// that can execute code. Instantiation involves allocating runtime resources,
    /// setting up the memory, and preparing the component for function calls.
    ///
    /// ## Returns
    ///
    /// * `Ok(ActorInstance)` - A ready-to-use instance of the WebAssembly component
    /// * `Err(anyhow::Error)` - If instantiation fails due to validation errors or resource limitations
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// # // ActorComponent and ActorInstance are internal types
    /// # use anyhow::Result;
    ///
    /// async fn create_instance(component: ActorComponent) -> Result<ActorInstance> {
    ///     // Instantiate the component
    ///     let instance = component.instantiate().await?;
    ///     
    ///     // Now we can call functions on the instance
    ///     // instance.register_function("some_interface", "some_function")?;
    ///     
    ///     Ok(instance)
    /// }
    /// ```
    ///
    /// ## Safety
    ///
    /// This method checks that the component can be properly instantiated with the given
    /// imports and that its structure is compatible with the Wasmtime runtime.
    ///
    /// ## Security
    ///
    /// Instantiation applies memory and execution limits to prevent denial-of-service
    /// attacks. Any instantiation errors are properly reported without exposing sensitive
    /// implementation details.
    ///
    /// ## Implementation Notes
    ///
    /// This method consumes the `ActorComponent` (takes ownership of it) since a component
    /// can only be instantiated once. The resulting `ActorInstance` contains the original
    /// component information.
    pub async fn instantiate(self) -> Result<ActorInstance> {
        let mut store = Store::new(&self.engine, self.actor_store.clone());

        let instance = self
            .linker
            .instantiate_async(&mut store, &self.component)
            .await
            .map_err(|e| WasmError::WasmError {
                context: "instantiation",
                message: e.to_string(),
            })?;

        Ok(ActorInstance {
            actor_component: self,
            instance,
            store,
            functions: HashMap::new(),
        })
    }
}

/// # WebAssembly Actor Instance
///
/// `ActorInstance` represents an instantiated WebAssembly component that is ready for
/// execution. It provides methods to register and call functions on the component.
///
/// ## Purpose
///
/// This struct is the primary interface for interacting with WebAssembly actors. It
/// manages the runtime state of the actor, provides type-safe function registration and
/// invocation, and maintains the connection between the host and the WebAssembly instance.
///
/// ## Example
///
/// ```rust,ignore
/// use theater::wasm::ActorInstance;
/// use anyhow::Result;
///
/// async fn interact_with_instance(mut instance: ActorInstance) -> Result<()> {
///     // Register a function with parameters and results
///     instance.register_function::<(String,), String>("my:interface", "greet")?;
///     
///     // Call the function
///     let state = None; // Initial state
///     let params = serde_json::to_vec(&("World",))?;
///     let (new_state, result): (Option<Vec<u8>>, Vec<u8>) = instance.call_function("my:interface.greet", state, params).await?;
///     
///     // Process the result
///     let greeting: String = serde_json::from_slice(&result)?;
///     println!("Greeting: {}", greeting);
///     
///     Ok(())
/// }
/// ```
///
/// ## Safety
///
/// This struct enforces type safety when registering and calling WebAssembly functions,
/// preventing type confusion errors and memory safety issues.
///
/// ## Security
///
/// `ActorInstance` maintains the isolation boundary between the host and WebAssembly,
/// ensuring that the WebAssembly code can only access resources and capabilities that
/// have been explicitly granted to it.
///
/// ## Implementation Notes
///
/// The instance maintains a registry of typed functions that have been registered,
/// allowing for efficient lookup and invocation. Function calls are asynchronous to
/// support non-blocking operation in the actor system.
pub struct ActorInstance {
    pub actor_component: ActorComponent,
    pub instance: Instance,
    pub store: Store<ActorStore>,
    pub functions: HashMap<String, Box<dyn TypedFunction>>,
}

impl ActorInstance {
    pub fn save_chain(&self) -> Result<()> {
        self.actor_component.actor_store.save_chain()
    }

    /// Checks if a specific function has been registered in this instance.
    ///
    /// ## Purpose
    ///
    /// This method allows checking whether a particular function is available
    /// before attempting to call it, which can help prevent runtime errors.
    ///
    /// ## Parameters
    ///
    /// * `name` - The fully qualified name of the function to check (typically "interface.function")
    ///
    /// ## Returns
    ///
    /// * `bool` - True if the function exists and is registered, false otherwise
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// # // ActorInstance is an internal type
    ///
    /// fn check_function(instance: &ActorInstance) {
    ///     if instance.has_function("theater:simple/actor.init") {
    ///         println!("The init function is available");
    ///     } else {
    ///         println!("The init function is not registered");
    ///     }
    /// }
    /// ```
    pub fn has_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Gets the unique identifier of this actor instance.
    ///
    /// ## Purpose
    ///
    /// This method returns the theater-specific identifier for this actor instance,
    /// which can be used for referencing the actor in logs, debugging, and actor-to-actor
    /// communication.
    ///
    /// ## Returns
    ///
    /// * `TheaterId` - The unique identifier for this actor
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// # // ActorInstance is an internal type
    ///
    /// fn log_actor_id(instance: &ActorInstance) {
    ///     let id = instance.id();
    ///     println!("Working with actor: {}", id);
    /// }
    /// ```
    pub fn id(&self) -> TheaterId {
        self.actor_component.actor_store.id.clone()
    }

    /// Calls a registered WebAssembly function with the given state and parameters.
    ///
    /// ## Purpose
    ///
    /// This is the core method for invoking actor functions. It takes serialized parameters,
    /// passes them to the WebAssembly function along with the current state, and returns
    /// both the updated state and the serialized result.
    ///
    /// ## Parameters
    ///
    /// * `name` - The fully qualified name of the function to call (typically "interface.function")
    /// * `state` - Optional binary state to pass to the function
    /// * `params` - Serialized parameters to pass to the function
    ///
    /// ## Returns
    ///
    /// * `Ok((Option<Vec<u8>>, Vec<u8>))` - A tuple with the updated state and the serialized result
    /// * `Err(anyhow::Error)` - If the function call fails or the function is not found
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// # // ActorInstance is an internal type
    /// # use anyhow::Result;
    ///
    /// async fn invoke_actor(mut instance: ActorInstance) -> Result<()> {
    ///     // Prepare parameters
    ///     let params = serde_json::to_vec(&("Hello, WebAssembly!",))?;
    ///     
    ///     // Call the function with no initial state
    ///     let (new_state, result) = instance.call_function(
    ///         "theater:simple/actor.init",
    ///         None,
    ///         params
    ///     ).await?;
    ///     
    ///     // Process the result
    ///     println!("Function returned {} bytes of data", result.len());
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## Safety
    ///
    /// This method safely handles the conversion between host and WebAssembly values,
    /// preventing memory corruption or unexpected behavior.
    ///
    /// ## Security
    ///
    /// The function call occurs within the WebAssembly sandbox, ensuring that malicious
    /// actor code cannot access unauthorized resources. State is passed by value, not
    /// by reference, preventing memory corruption vulnerabilities.
    ///
    /// ## Implementation Notes
    ///
    /// Function calls are asynchronous to allow for non-blocking operation in the actor
    /// system. The state is passed as a separate parameter to allow the function to
    /// update it and return the new version.
    pub async fn call_function(
        &mut self,
        name: &str,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        let func = match self.functions.get(name) {
            Some(f) => f,
            None => {
                error!(
                    "Function '{}' not found in functions table. Available functions: {:?}",
                    name,
                    self.functions.keys().collect::<Vec<_>>()
                );
                return Err(anyhow::anyhow!(
                    "Function '{}' not found in functions table",
                    name
                ));
            }
        };
        func.call_func(&mut self.store, state, params).await
    }

    /// Registers a WebAssembly function with parameters and results for later invocation.
    ///
    /// ## Purpose
    ///
    /// This method locates a function export in the WebAssembly component and creates a
    /// type-safe wrapper for calling it. The wrapper handles serialization and deserialization
    /// of parameters and results using the provided generic types.
    ///
    /// ## Type Parameters
    ///
    /// * `P` - The parameter type, must implement necessary trait bounds for WebAssembly conversion
    /// * `R` - The result type, must implement necessary trait bounds for WebAssembly conversion
    ///
    /// ## Parameters
    ///
    /// * `interface` - The name of the interface containing the function
    /// * `function_name` - The name of the function to register
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - If the function was successfully registered
    /// * `Err(anyhow::Error)` - If the function could not be found or registered
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// # // ActorInstance is an internal type
    /// # use anyhow::Result;
    ///
    /// fn register_functions(mut instance: &mut ActorInstance) -> Result<()> {
    ///     // Register a function with string parameter and result
    ///     instance.register_function::<(String,), String>("example:greeter/hello", "greet")?;
    ///     
    ///     // Register a function with complex parameter and result types
    ///     // For custom types, define them first:
///     // struct User { name: String }
///     // struct Response { success: bool }
///     // instance.register_function::<(User,), Response>("example:api/users", "create_user")?;
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## Safety
    ///
    /// This method verifies that the function exists and has the expected signature before
    /// registering it, preventing type confusion errors at call time.
    ///
    /// ## Implementation Notes
    ///
    /// The registered function is stored in the instance's functions map with a fully
    /// qualified name combining the interface and function name (e.g., "interface.function").
    /// The wrapped function handles conversion between Rust types and WebAssembly values.
    pub fn register_function<P, R>(&mut self, interface: &str, function_name: &str) -> Result<()>
    where
        P: ComponentType + Lower + ComponentNamedList + Send + Sync + 'static,
        R: ComponentType + Lift + ComponentNamedList + Send + Sync + 'static,
        for<'de> P: Deserialize<'de>,
        R: Serialize,
    {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let name = format!("{}.{}", interface, function_name);
        let func =
            TypedComponentFunction::<P, R>::new(&mut self.store, &self.instance, export_index)
                .expect("Failed to create typed function");
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }

    pub fn register_function_no_params<R>(
        &mut self,
        interface: &str,
        function_name: &str,
    ) -> Result<()>
    where
        R: ComponentType + Lift + ComponentNamedList + Send + Sync + 'static,
        R: Serialize,
    {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let name = format!("{}.{}", interface, function_name);
        let func =
            TypedComponentFunctionNoParams::<R>::new(&mut self.store, &self.instance, export_index)
                .expect("Failed to create typed function");
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }

    pub fn register_function_no_result<P>(
        &mut self,
        interface: &str,
        function_name: &str,
    ) -> Result<()>
    where
        P: ComponentType + Lower + ComponentNamedList + Send + Sync + 'static,
        for<'de> P: Deserialize<'de>,
    {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let name = format!("{}.{}", interface, function_name);
        let func = TypedComponentFunctionNoResult::<P>::new(
            &mut self.store,
            &self.instance,
            export_index,
        )?;
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }

    pub fn register_function_no_params_no_result(
        &mut self,
        interface: &str,
        function_name: &str,
    ) -> Result<()> {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        let name = format!("{}.{}", interface, function_name);
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let func = TypedComponentFunctionNoParamsNoResult::new(
            &mut self.store,
            &self.instance,
            export_index,
        )?;
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }
}

pub struct TypedComponentFunction<P, R>
where
    P: ComponentNamedList,
    R: ComponentNamedList,
{
    func: TypedFunc<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>, R), String>,)>,
}

impl<P, R> TypedComponentFunction<P, R>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static,
    R: ComponentNamedList + Lift + Sync + Send + 'static,
{
    pub fn new(
        store: &mut Store<ActorStore>,
        instance: &Instance,
        export_index: ComponentExportIndex,
    ) -> Result<Self> {
        let func = instance
            .get_func(&mut *store, export_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "function retrieval",
                message: "Function not found".to_string(),
            })?;

        // Convert the Func to a TypedFunc with the correct signature
        let typed_func = func
            .typed::<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>, R), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunction { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: P,
    ) -> Result<(Option<Vec<u8>>, R), String> {
        match self.func.call_async(&mut *store, (state, params)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res.0,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!("Failed to call WebAssembly function: {}", e);
                error!("{}", error_msg);
                return Err(error_msg);
            }
        }
    }
}

/// # Type-Safe WebAssembly Function Interface
///
/// This trait provides a unified interface for calling WebAssembly functions
/// with different parameter and return value signatures. It abstracts away the
/// details of type conversion and WebAssembly function invocation.
///
/// ## Purpose
///
/// `TypedFunction` enables a uniform calling convention for all WebAssembly functions,
/// regardless of their specific parameter and return types. This allows the actor system
/// to work with arbitrary functions through a common interface while maintaining type safety.
///
/// ## Implementation Notes
///
/// Implementations of this trait handle:
/// * Serialization/deserialization of parameters and results
/// * Type conversion between Rust and WebAssembly values
/// * Error handling for WebAssembly calls
/// * State management for stateful actor functions
///
/// The trait is implemented for various function signature patterns, including
/// functions with parameters and results, functions with only parameters,
/// functions with only results, and functions with neither.
///
/// ## Safety
///
/// This trait ensures type safety when calling WebAssembly functions by handling
/// the conversion between Rust types and WebAssembly values in a consistent way.
/// The `Send + Sync + 'static` bounds ensure that implementations can be safely
/// used across thread boundaries and stored in collections.
pub trait TypedFunction: Send + Sync + 'static {
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>>;
}

impl<P, R> TypedFunction for TypedComponentFunction<P, R>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            // This is a simplified conversion - you'll need to implement actual conversion logic
            // from Vec<u8> to P and from R to Vec<u8> based on your serialization format
            let params_deserialized: P = serde_json::from_slice(&params)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize params: {}", e))?;

            match self.call_func(store, state, params_deserialized).await {
                Ok((new_state, result)) => {
                    let result_serialized = serde_json::to_vec(&result)
                        .map_err(|e| anyhow::anyhow!("Failed to serialize result: {}", e))?;

                    Ok((new_state, result_serialized))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}

pub struct TypedComponentFunctionNoParams<R>
where
    R: ComponentNamedList,
{
    func: TypedFunc<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>, R),), String>,)>,
}

impl<R> TypedComponentFunctionNoParams<R>
where
    R: ComponentNamedList + Lift + Sync + Send + 'static,
{
    pub fn new(
        store: &mut Store<ActorStore>,
        instance: &Instance,
        export_index: ComponentExportIndex,
    ) -> Result<Self> {
        let func = instance
            .get_func(&mut *store, export_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "function retrieval",
                message: "Function not found".to_string(),
            })?;

        let typed_func = func
            .typed::<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>, R),), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunctionNoParams { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
    ) -> Result<((Option<Vec<u8>>, R),), String> {
        let result = match self.func.call_async(&mut *store, (state,)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!("Failed to call WebAssembly function (no params): {}", e);
                error!("{}", error_msg);
                return Err(error_msg);
            }
        };
        result.0
    }
}

impl<R> TypedFunction for TypedComponentFunctionNoParams<R>
where
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        _params: Vec<u8>, // Ignore params
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            match self.call_func(store, state).await {
                Ok(((new_state, result),)) => {
                    let result_serialized = serde_json::to_vec(&result)
                        .map_err(|e| anyhow::anyhow!("Failed to serialize result: {}", e))?;

                    Ok((new_state, result_serialized))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}

pub struct TypedComponentFunctionNoResult<P>
where
    P: ComponentNamedList,
{
    func: TypedFunc<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>,), String>,)>,
}

impl<P> TypedComponentFunctionNoResult<P>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static,
{
    pub fn new(
        store: &mut Store<ActorStore>,
        instance: &Instance,
        export_index: ComponentExportIndex,
    ) -> Result<Self> {
        let func = instance
            .get_func(&mut *store, export_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "function retrieval",
                message: "Function not found".to_string(),
            })?;

        let typed_func = func
            .typed::<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>,), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunctionNoResult { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: P,
    ) -> Result<(Option<Vec<u8>>,), String> {
        let result = match self.func.call_async(&mut *store, (state, params)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!("Failed to call WebAssembly function (no result): {}", e);
                error!("{}", error_msg);
                return Err(error_msg);
            }
        };
        result.0
    }
}

impl<P> TypedFunction for TypedComponentFunctionNoResult<P>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            let params_deserialized: P = serde_json::from_slice(&params)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize params: {}", e))?;

            match self.call_func(store, state, params_deserialized).await {
                Ok((new_state,)) => {
                    // Return empty Vec<u8> as result
                    Ok((new_state, serde_json::to_vec(&()).unwrap()))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}

pub struct TypedComponentFunctionNoParamsNoResult {
    func: TypedFunc<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>,),), String>,)>,
}

impl TypedComponentFunctionNoParamsNoResult {
    pub fn new(
        store: &mut Store<ActorStore>,
        instance: &Instance,
        export_index: ComponentExportIndex,
    ) -> Result<Self> {
        let func = instance
            .get_func(&mut *store, export_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "function retrieval",
                message: "Function not found".to_string(),
            })?;

        let typed_func = func
            .typed::<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>,),), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunctionNoParamsNoResult { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
    ) -> Result<((Option<Vec<u8>>,),), String> {
        let result = match self.func.call_async(&mut *store, (state,)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!(
                    "Failed to call WebAssembly function (no params, no result): {}",
                    e
                );
                error!("{}", error_msg);
                return Err(error_msg);
            }
        };
        result.0
    }
}

impl TypedFunction for TypedComponentFunctionNoParamsNoResult {
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        _params: Vec<u8>, // Ignore params
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            match self.call_func(store, state).await {
                Ok(((new_state,),)) => {
                    // Return empty Vec<u8> as result
                    Ok((new_state, Vec::new()))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}
