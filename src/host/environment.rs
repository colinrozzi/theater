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
        mut shutdown_receiver: ShutdownReceiver,
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
            .instance("ntwk:theater/environment")
            .expect("Could not instantiate ntwk:theater/environment");

        let config = self.config.clone();
        
        // get-var implementation
        interface.func_wrap(
            "get-var",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (var_name,): (String,)| -> Result<(Option<String>,)> {
                let is_allowed = config.is_variable_allowed(&var_name);
                
                if !is_allowed {
                    // Record access denial
                    let event_data = EnvironmentEventData {
                        operation: "get-var".to_string(),
                        variable_name: var_name.clone(),
                        success: false,
                        value_found: false,
                        timestamp: chrono::Utc::now(),
                    };
                    
                    let chain_event = ChainEvent {
                        data: EventData::Environment(event_data),
                        chain_id: ctx.data().chain_id.clone(),
                        sequence_number: ctx.data_mut().get_next_sequence_number(),
                        timestamp: chrono::Utc::now(),
                    };
                    
                    ctx.data_mut().add_event(chain_event);
                    return Ok((None,));
                }

                let value = env::var(&var_name).ok();
                let value_found = value.is_some();

                // Record the access attempt
                let event_data = EnvironmentEventData {
                    operation: "get-var".to_string(),
                    variable_name: var_name,
                    success: true,
                    value_found,
                    timestamp: chrono::Utc::now(),
                };
                
                let chain_event = ChainEvent {
                    data: EventData::Environment(event_data),
                    chain_id: ctx.data().chain_id.clone(),
                    sequence_number: ctx.data_mut().get_next_sequence_number(),
                    timestamp: chrono::Utc::now(),
                };
                
                ctx.data_mut().add_event(chain_event);

                Ok((value,))
            },
        )?;

        let config_clone = self.config.clone();

        // exists implementation
        interface.func_wrap(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (var_name,): (String,)| -> Result<(bool,)> {
                let is_allowed = config_clone.is_variable_allowed(&var_name);
                
                if !is_allowed {
                    // Record access denial
                    let event_data = EnvironmentEventData {
                        operation: "exists".to_string(),
                        variable_name: var_name,
                        success: false,
                        value_found: false,
                        timestamp: chrono::Utc::now(),
                    };
                    
                    let chain_event = ChainEvent {
                        data: EventData::Environment(event_data),
                        chain_id: ctx.data().chain_id.clone(),
                        sequence_number: ctx.data_mut().get_next_sequence_number(),
                        timestamp: chrono::Utc::now(),
                    };
                    
                    ctx.data_mut().add_event(chain_event);
                    return Ok((false,));
                }

                let exists = env::var(&var_name).is_ok();

                // Record the check
                let event_data = EnvironmentEventData {
                    operation: "exists".to_string(),
                    variable_name: var_name,
                    success: true,
                    value_found: exists,
                    timestamp: chrono::Utc::now(),
                };
                
                let chain_event = ChainEvent {
                    data: EventData::Environment(event_data),
                    chain_id: ctx.data().chain_id.clone(),
                    sequence_number: ctx.data_mut().get_next_sequence_number(),
                    timestamp: chrono::Utc::now(),
                };
                
                ctx.data_mut().add_event(chain_event);

                Ok((exists,))
            },
        )?;

        let config_clone2 = self.config.clone();

        // list-vars implementation
        interface.func_wrap(
            "list-vars",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (): ()| -> Result<(Vec<(String, String)>,)> {
                if !config_clone2.allow_list_all {
                    // Record denied list attempt
                    let event_data = EnvironmentEventData {
                        operation: "list-vars".to_string(),
                        variable_name: "(list-all disabled)".to_string(),
                        success: false,
                        value_found: false,
                        timestamp: chrono::Utc::now(),
                    };
                    
                    let chain_event = ChainEvent {
                        data: EventData::Environment(event_data),
                        chain_id: ctx.data().chain_id.clone(),
                        sequence_number: ctx.data_mut().get_next_sequence_number(),
                        timestamp: chrono::Utc::now(),
                    };
                    
                    ctx.data_mut().add_event(chain_event);
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
                let event_data = EnvironmentEventData {
                    operation: "list-vars".to_string(),
                    variable_name: format!("(returned {} variables)", count),
                    success: true,
                    value_found: count > 0,
                    timestamp: chrono::Utc::now(),
                };
                
                let chain_event = ChainEvent {
                    data: EventData::Environment(event_data),
                    chain_id: ctx.data().chain_id.clone(),
                    sequence_number: ctx.data_mut().get_next_sequence_number(),
                    timestamp: chrono::Utc::now(),
                };
                
                ctx.data_mut().add_event(chain_event);

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
