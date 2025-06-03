use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::config::EnvironmentHandlerConfig;
use crate::events::environment::EnvironmentEventData;
use crate::events::{ChainEventData, EventData};
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
}

impl EnvironmentHost {
    pub fn new(config: EnvironmentHandlerConfig) -> Self {
        Self {
            config,
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

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up environment host functions (read-only)");

        let mut interface = actor_component
            .linker
            .instance("theater:simple/environment")
            .expect("Could not instantiate theater:simple/environment");

        let config = self.config.clone();
        
        // get-var implementation
        interface.func_wrap(
            "get-var",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (var_name,): (String,)| -> Result<(Option<String>,)> {
                let now = Utc::now().timestamp_millis() as u64;
                let is_allowed = config.is_variable_allowed(&var_name);
                
                if !is_allowed {
                    // Record access denial
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/environment/get-var".to_string(),
                        data: EventData::Environment(EnvironmentEventData {
                            operation: "get-var".to_string(),
                            variable_name: var_name.clone(),
                            success: false,
                            value_found: false,
                            timestamp: chrono::Utc::now(),
                        }),
                        timestamp: now,
                        description: Some(format!("Access denied for environment variable: {}", var_name)),
                    });
                    return Ok((None,));
                }

                let value = env::var(&var_name).ok();
                let value_found = value.is_some();

                // Record the access attempt
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/environment/get-var".to_string(),
                    data: EventData::Environment(EnvironmentEventData {
                        operation: "get-var".to_string(),
                        variable_name: var_name.clone(),
                        success: true,
                        value_found,
                        timestamp: chrono::Utc::now(),
                    }),
                    timestamp: now,
                    description: Some(format!("Environment variable access: {} (found: {})", var_name, value_found)),
                });

                Ok((value,))
            },
        )?;

        let config_clone = self.config.clone();

        // exists implementation
        interface.func_wrap(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (var_name,): (String,)| -> Result<(bool,)> {
                let now = Utc::now().timestamp_millis() as u64;
                let is_allowed = config_clone.is_variable_allowed(&var_name);
                
                if !is_allowed {
                    // Record access denial
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/environment/exists".to_string(),
                        data: EventData::Environment(EnvironmentEventData {
                            operation: "exists".to_string(),
                            variable_name: var_name.clone(),
                            success: false,
                            value_found: false,
                            timestamp: chrono::Utc::now(),
                        }),
                        timestamp: now,
                        description: Some(format!("Access denied for environment variable exists check: {}", var_name)),
                    });
                    return Ok((false,));
                }

                let exists = env::var(&var_name).is_ok();

                // Record the check
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/environment/exists".to_string(),
                    data: EventData::Environment(EnvironmentEventData {
                        operation: "exists".to_string(),
                        variable_name: var_name.clone(),
                        success: true,
                        value_found: exists,
                        timestamp: chrono::Utc::now(),
                    }),
                    timestamp: now,
                    description: Some(format!("Environment variable exists check: {} (exists: {})", var_name, exists)),
                });

                Ok((exists,))
            },
        )?;

        let config_clone2 = self.config.clone();

        // list-vars implementation
        interface.func_wrap(
            "list-vars",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (): ()| -> Result<(Vec<(String, String)>,)> {
                let now = Utc::now().timestamp_millis() as u64;
                
                if !config_clone2.allow_list_all {
                    // Record denied list attempt
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/environment/list-vars".to_string(),
                        data: EventData::Environment(EnvironmentEventData {
                            operation: "list-vars".to_string(),
                            variable_name: "(list-all disabled)".to_string(),
                            success: false,
                            value_found: false,
                            timestamp: chrono::Utc::now(),
                        }),
                        timestamp: now,
                        description: Some("Environment variable listing denied - allow_list_all is false".to_string()),
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
                    data: EventData::Environment(EnvironmentEventData {
                        operation: "list-vars".to_string(),
                        variable_name: format!("(returned {} variables)", count),
                        success: true,
                        value_found: count > 0,
                        timestamp: chrono::Utc::now(),
                    }),
                    timestamp: now,
                    description: Some(format!("Environment variable listing returned {} accessible variables", count)),
                });

                Ok((accessible_vars,))
            },
        )?;

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        // Environment handler (read-only) doesn't need export functions
        Ok(())
    }
}
