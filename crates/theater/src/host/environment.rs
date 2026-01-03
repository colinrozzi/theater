use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::config::actor_manifest::EnvironmentHandlerConfig;
use crate::config::enforcement::PermissionChecker;
// use crate::events::environment::EnvironmentEventData;
use crate::events::EventData;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use chrono::Utc;
use std::env;
use thiserror::Error;
use tracing::info;
use wasmtime::StoreContextMut;

#[derive(Error, Debug)]
pub enum EnvironmentError {
    #[error("Access denied for environment variable: {0}")]
    AccessDenied(String),

    #[error("Environment variable not found: {0}")]
    VariableNotFound(String),

    #[error("Invalid variable name: {0}")]
    InvalidVariableName(String),
}

pub struct EnvironmentHost {
    config: EnvironmentHandlerConfig,
    permissions: Option<crate::config::permissions::EnvironmentPermissions>,
}

impl EnvironmentHost {
    pub fn new(
        config: EnvironmentHandlerConfig,
        permissions: Option<crate::config::permissions::EnvironmentPermissions>,
    ) -> Self {
        Self {
            config,
            permissions,
        }
    }

    pub async fn start(
        &mut self,
        _actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting Environment handler (read-only)");

        // Wait for shutdown signal
        shutdown_receiver.wait_for_shutdown().await;
        info!("Environment handler shutting down");
        Ok(())
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent<EventData>) -> Result<()> {
        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "environment-setup".to_string(),
            data: EventData::Environment(EnvironmentEventData::HandlerSetupStart),
            description: Some("Starting environment host function setup".to_string()),
        });

        info!("Setting up environment host functions (read-only)");

        let mut interface = match actor_component
            .linker
            .instance("theater:simple/environment")
        {
            Ok(interface) => {
                // Record successful linker instance creation
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "environment-setup".to_string(),
                    data: EventData::Environment(EnvironmentEventData::LinkerInstanceSuccess),
                    description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "environment-setup".to_string(),
                    data: EventData::Environment(EnvironmentEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/environment: {}",
                    e
                ));
            }
        };

        let _config = self.config.clone();
        let permissions = self.permissions.clone();

        // get-var implementation
        interface.func_wrap(
            "get-var",
            move |mut ctx: StoreContextMut<'_, ActorStore<EventData>>,
                  (var_name,): (String,)|
                  -> Result<(Option<String>,)> {
                let now = Utc::now().timestamp_millis() as u64;

                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_env_var_access(&permissions, &var_name) {
                    // Record permission denied event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/environment/permission-denied".to_string(),
                        data: EventData::Environment(EnvironmentEventData::PermissionDenied {
                            operation: "get-var".to_string(),
                            variable_name: var_name.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: now,
                        description: Some(format!(
                            "Permission denied for environment variable access: {}",
                            e
                        )),
                    });
                    return Ok((None,));
                }

                let value = env::var(&var_name).ok();
                let value_found = value.is_some();

                // Record the access attempt
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/environment/get-var".to_string(),
                    data: EventData::Environment(EnvironmentEventData::GetVar {
                        variable_name: var_name.clone(),
                        success: true,
                        value_found,
                        timestamp: chrono::Utc::now(),
                    }),
                    timestamp: now,
                    description: Some(format!(
                        "Environment variable access: {} (found: {})",
                        var_name, value_found
                    )),
                });

                Ok((value,))
            },
        )?;

        let permissions_clone = self.permissions.clone();

        // exists implementation
        interface.func_wrap(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (var_name,): (String,)|
                  -> Result<(bool,)> {
                let now = Utc::now().timestamp_millis() as u64;

                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) =
                    PermissionChecker::check_env_var_access(&permissions_clone, &var_name)
                {
                    // Record permission denied event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/environment/permission-denied".to_string(),
                        data: EventData::Environment(EnvironmentEventData::PermissionDenied {
                            operation: "exists".to_string(),
                            variable_name: var_name.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: now,
                        description: Some(format!(
                            "Permission denied for environment variable exists check: {}",
                            e
                        )),
                    });
                    return Ok((false,));
                }

                let exists = env::var(&var_name).is_ok();

                // Record the check
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/environment/exists".to_string(),
                    data: EventData::Environment(EnvironmentEventData::GetVar {
                        variable_name: var_name.clone(),
                        success: true,
                        value_found: exists,
                        timestamp: chrono::Utc::now(),
                    }),
                    timestamp: now,
                    description: Some(format!(
                        "Environment variable exists check: {} (exists: {})",
                        var_name, exists
                    )),
                });

                Ok((exists,))
            },
        )?;

        let config_clone2 = self.config.clone();

        // list-vars implementation
        interface.func_wrap(
            "list-vars",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (): ()|
                  -> Result<(Vec<(String, String)>,)> {
                let now = Utc::now().timestamp_millis() as u64;

                if !config_clone2.allow_list_all {
                    // Record denied list attempt
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/environment/list-vars".to_string(),
                        data: EventData::Environment(EnvironmentEventData::PermissionDenied {
                            operation: "list-vars".to_string(),
                            variable_name: "(list-all disabled)".to_string(),
                            reason: "allow_list_all is false".to_string(),
                        }),
                        timestamp: now,
                        description: Some(
                            "Environment variable listing denied - allow_list_all is false"
                                .to_string(),
                        ),
                    });
                    return Ok((Vec::new(),));
                }

                let mut accessible_vars = Vec::new();

                for (key, value) in env::vars() {
                    if config_clone2.is_variable_allowed(&key) {
                        accessible_vars.push((key, value));
                    }
                }

                // Record the list operation
                let count = accessible_vars.len();
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/environment/list-vars".to_string(),
                    data: EventData::Environment(EnvironmentEventData::GetVar {
                        variable_name: format!("(returned {} variables)", count),
                        success: true,
                        value_found: count > 0,
                        timestamp: chrono::Utc::now(),
                    }),
                    timestamp: now,
                    description: Some(format!(
                        "Environment variable listing returned {} accessible variables",
                        count
                    )),
                });

                Ok((accessible_vars,))
            },
        )?;

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "environment-setup".to_string(),
            data: EventData::Environment(EnvironmentEventData::HandlerSetupSuccess),
            description: Some(
                "Environment host functions setup completed successfully".to_string(),
            ),
        });

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance<EventData>) -> Result<()> {
        // Environment handler (read-only) doesn't need export functions
        Ok(())
    }
}
