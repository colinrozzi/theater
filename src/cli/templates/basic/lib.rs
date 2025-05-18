mod bindings;

use crate::bindings::exports::ntwk::theater::actor::Guest;
use crate::bindings::exports::ntwk::theater::message_server_client::Guest as MessageServerClient;
use crate::bindings::ntwk::theater::runtime::log;
use crate::bindings::ntwk::theater::types::State;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct AppState {
    count: u32,
    messages: Vec<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            count: 0,
            messages: Vec::new(),
        }
    }
}

struct Actor;
impl Guest for Actor {
    fn init(
        init_state_bytes: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} actor");
        let (self_id,) = params;
        log(format!("Actor ID: {}", self_id));

        let app_state = AppState::default();
        log("Created default app state");
        let state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;

        // Create the initial state
        let new_state = Some(state_bytes);

        Ok((new_state,))
    }
}

bindings::export!(Actor with_types_in bindings);
