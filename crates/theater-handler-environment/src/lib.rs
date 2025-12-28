//! # Environment Variable Handler
//!
//! Provides environment variable access to WebAssembly actors in the Theater system.
//! This handler allows actors to read environment variables while maintaining security
//! boundaries and permission controls.
//!
//! ## Features
//!
//! - **Read-only access**: Actors can read but not modify environment variables
//! - **Permission-based access**: Control which variables actors can access
//! - **Variable listing**: Optionally allow actors to list all accessible variables
//! - **Event logging**: All environment variable accesses are logged to the chain
//!
//! ## Example
//!
//! ```rust,no_run
//! use theater_handler_environment::EnvironmentHandler;
//! use theater::config::actor_manifest::EnvironmentHandlerConfig;
//!
//! let config = EnvironmentHandlerConfig {
//!     allowed_vars: None,
//!     denied_vars: None,
//!     allow_list_all: false,
//!     allowed_prefixes: None,
//! };
//! let handler = EnvironmentHandler::new(config, None);
//! ```

// Export events module for applications to use
pub mod events;
pub use events::EnvironmentEventData;

use anyhow::Result;
use chrono::Utc;
use std::env;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tracing::info;
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::EnvironmentHandlerConfig;
use theater::config::enforcement::PermissionChecker;
use theater::config::permissions::EnvironmentPermissions;
use theater::events::EventPayload;
use theater::handler::{Handler, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

/// Error types for environment operations
#[derive(Error, Debug)]
pub enum EnvironmentError {
    #[error("Access denied for environment variable: {0}")]
    AccessDenied(String),

    #[error("Environment variable not found: {0}")]
    VariableNotFound(String),

    #[error("Invalid variable name: {0}")]
    InvalidVariableName(String),
}

/// Host for providing environment variable access to WebAssembly actors
#[derive(Clone)]
pub struct EnvironmentHandler {
    config: EnvironmentHandlerConfig,
    permissions: Option<EnvironmentPermissions>,
}

impl EnvironmentHandler {
    /// Create a new environment handler
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for the environment handler
    /// * `permissions` - Optional permissions controlling variable access
    pub fn new(config: EnvironmentHandlerConfig, permissions: Option<EnvironmentPermissions>) -> Self {
        // If no permissions provided, create them from the config
        let permissions = permissions.or_else(|| {
            Some(EnvironmentPermissions {
                allowed_vars: config.allowed_vars.clone(),
                denied_vars: config.denied_vars.clone(),
                allow_list_all: config.allow_list_all,
                allowed_prefixes: config.allowed_prefixes.clone(),
            })
        });

        Self {
            config,
            permissions,
        }
    }
}

impl<E> Handler<E> for EnvironmentHandler
where
    E: EventPayload + Clone + From<EnvironmentEventData>,
{
    fn create_instance(&self) -> Box<dyn Handler<E>> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance<E>,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Starting Environment handler (read-only)");

        Box::pin(async move {
            // Environment handler doesn't need a background task, but we should wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;
            info!("Environment handler received shutdown signal");
            info!("Environment handler shut down");
            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent<E>,
    ) -> Result<()> {
        // Clone what we need for the closures
        let permissions_get = self.permissions.clone();
        let permissions_exists = self.permissions.clone();
        let config_list = self.config.clone();

        // Record setup start
        actor_component.actor_store.record_handler_event(
            "environment-setup".to_string(),
            EnvironmentEventData::HandlerSetupStart,
            Some("Starting environment host function setup".to_string()),
        );

        info!("Setting up environment host functions (read-only)");

        let mut interface = match actor_component
            .linker
            .instance("theater:simple/environment")
        {
            Ok(interface) => {
                // Record successful linker instance creation
                actor_component.actor_store.record_handler_event(
                    "environment-setup".to_string(),
                    EnvironmentEventData::LinkerInstanceSuccess,
                    Some("Successfully created linker instance".to_string()),
                );
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_handler_event(
                    "environment-setup".to_string(),
                    EnvironmentEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    },
                    Some(format!("Failed to create linker instance: {}", e)),
                );
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/environment: {}",
                    e
                ));
            }
        };

        // get-var implementation
        interface.func_wrap(
            "get-var",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (var_name,): (String,)|
                  -> Result<(Option<String>,)> {
                let _now = Utc::now().timestamp_millis() as u64;

                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_env_var_access(&permissions_get, &var_name) {
                    // Record permission denied event
                    ctx.data_mut().record_handler_event(
                        "theater:simple/environment/permission-denied".to_string(),
                        EnvironmentEventData::PermissionDenied {
                            operation: "get-var".to_string(),
                            variable_name: var_name.clone(),
                            reason: e.to_string(),
                        },
                        Some(format!(
                            "Permission denied for environment variable access: {}",
                            e
                        )),
                    );
                    return Ok((None,));
                }

                let value = env::var(&var_name).ok();
                let value_found = value.is_some();

                // Record the access attempt
                ctx.data_mut().record_handler_event(
                    "theater:simple/environment/get-var".to_string(),
                    EnvironmentEventData::GetVar {
                        variable_name: var_name.clone(),
                        success: true,
                        value_found,
                        timestamp: chrono::Utc::now(),
                    },
                    Some(format!(
                        "Environment variable access: {} (found: {})",
                        var_name, value_found
                    )),
                );

                Ok((value,))
            },
        )?;

        // exists implementation
        interface.func_wrap(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (var_name,): (String,)|
                  -> Result<(bool,)> {
                let _now = Utc::now().timestamp_millis() as u64;

                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) =
                    PermissionChecker::check_env_var_access(&permissions_exists, &var_name)
                {
                    // Record permission denied event
                    ctx.data_mut().record_handler_event(
                        "theater:simple/environment/permission-denied".to_string(),
                        EnvironmentEventData::PermissionDenied {
                            operation: "exists".to_string(),
                            variable_name: var_name.clone(),
                            reason: e.to_string(),
                        },
                        Some(format!(
                            "Permission denied for environment variable exists check: {}",
                            e
                        )),
                    );
                    return Ok((false,));
                }

                let exists = env::var(&var_name).is_ok();

                // Record the check
                ctx.data_mut().record_handler_event(
                    "theater:simple/environment/exists".to_string(),
                    EnvironmentEventData::GetVar {
                        variable_name: var_name.clone(),
                        success: true,
                        value_found: exists,
                        timestamp: chrono::Utc::now(),
                    },
                    Some(format!(
                        "Environment variable exists check: {} (exists: {})",
                        var_name, exists
                    )),
                );

                Ok((exists,))
            },
        )?;

        // list-vars implementation
        interface.func_wrap(
            "list-vars",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  ()|
                  -> Result<(Vec<(String, String)>,)> {
                let _now = Utc::now().timestamp_millis() as u64;

                if !config_list.allow_list_all {
                    // Record denied list attempt
                    ctx.data_mut().record_handler_event(
                        "theater:simple/environment/list-vars".to_string(),
                        EnvironmentEventData::PermissionDenied {
                            operation: "list-vars".to_string(),
                            variable_name: "(list-all disabled)".to_string(),
                            reason: "allow_list_all is false".to_string(),
                        },
                        Some(
                            "Environment variable listing denied - allow_list_all is false"
                                .to_string(),
                        ),
                    );
                    return Ok((Vec::new(),));
                }

                let mut accessible_vars = Vec::new();

                for (key, value) in env::vars() {
                    if config_list.is_variable_allowed(&key) {
                        accessible_vars.push((key, value));
                    }
                }

                // Record the list operation
                let count = accessible_vars.len();
                ctx.data_mut().record_handler_event(
                    "theater:simple/environment/list-vars".to_string(),
                    EnvironmentEventData::GetVar {
                        variable_name: format!("(returned {} variables)", count),
                        success: true,
                        value_found: count > 0,
                        timestamp: chrono::Utc::now(),
                    },
                    Some(format!(
                        "Environment variable listing returned {} accessible variables",
                        count
                    )),
                );

                Ok((accessible_vars,))
            },
        )?;

        // Record overall setup completion
        actor_component.actor_store.record_handler_event(
            "environment-setup".to_string(),
            EnvironmentEventData::HandlerSetupSuccess,
            Some(
                "Environment host functions setup completed successfully".to_string(),
            ),
        );

        Ok(())
    }

    fn add_export_functions(&self, _actor_instance: &mut ActorInstance<E>) -> Result<()> {
        // Environment handler (read-only) doesn't need export functions
        Ok(())
    }

    fn name(&self) -> &str {
        "environment"
    }

    fn imports(&self) -> Option<String> {
        Some("theater:simple/environment".to_string())
    }

    fn exports(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_creation() {
        let config = EnvironmentHandlerConfig {
            allowed_vars: None,
            denied_vars: None,
            allow_list_all: false,
            allowed_prefixes: None,
        };
        let handler = EnvironmentHandler::new(config, None);
        assert_eq!(handler.name(), "environment");
        assert_eq!(handler.imports(), Some("theater:simple/environment".to_string()));
        assert_eq!(handler.exports(), None);
    }

    #[test]
    fn test_handler_clone() {
        let config = EnvironmentHandlerConfig {
            allowed_vars: None,
            denied_vars: None,
            allow_list_all: true,
            allowed_prefixes: None,
        };
        let handler = EnvironmentHandler::new(config, None);
        let cloned = handler.clone();
        assert_eq!(cloned.config.allow_list_all, true);
    }
}
