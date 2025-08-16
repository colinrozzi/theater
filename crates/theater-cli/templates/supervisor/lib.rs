#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::exports::theater::simple::supervisor_handlers::Guest as SupervisorHandlers;
use bindings::theater::simple::runtime::log;
use bindings::theater::simple::supervisor;
use bindings::theater::simple::types::WitActorError;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct SupervisorState {
    children: Vec<String>,
    restart_count: u32,
}

struct Component;

impl Guest for Component {
    fn init(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} supervisor actor");
        let (self_id,) = params;
        log(&format!("Supervisor ID: {}", &self_id));

        // Parse existing state or create new
        let supervisor_state = match state {
            Some(bytes) => {
                serde_json::from_slice::<SupervisorState>(&bytes)
                    .unwrap_or_else(|_| SupervisorState::default())
            }
            None => SupervisorState::default(),
        };

        // Serialize state back
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        Ok((Some(new_state),))
    }
}

impl SupervisorHandlers for Component {
    fn handle_child_error(
        state: Option<Vec<u8>>,
        params: (String, WitActorError),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (child_id, error) = params;
        log(&format!("Child actor {} error: {:?}", child_id, error));
        
        // Parse state
        let mut supervisor_state: SupervisorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => SupervisorState::default(),
        };
        
        // Implement restart strategy
        supervisor_state.restart_count += 1;
        log(&format!("Restarting child {} (attempt {})", child_id, supervisor_state.restart_count));
        
        // TODO: Implement actual restart logic using supervisor interface
        
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        
        Ok((Some(new_state),))
    }

    fn handle_child_exit(
        state: Option<Vec<u8>>,
        params: (String, Option<Vec<u8>>),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (child_id, _exit_data) = params;
        log(&format!("Child actor exited: {}", child_id));
        
        // Parse state
        let mut supervisor_state: SupervisorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => SupervisorState::default(),
        };
        
        // Remove child from tracking
        supervisor_state.children.retain(|id| id != &child_id);
        
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        
        Ok((Some(new_state),))
    }

    fn handle_child_external_stop(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (child_id,) = params;
        log(&format!("Child actor externally stopped: {}", child_id));
        
        // Parse state
        let mut supervisor_state: SupervisorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => SupervisorState::default(),
        };
        
        // Remove child from tracking
        supervisor_state.children.retain(|id| id != &child_id);
        
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        
        Ok((Some(new_state),))
    }
}

bindings::export!(Component with_types_in bindings);
