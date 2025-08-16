#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::theater::simple::runtime::{log, shutdown};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct ActorState {
    counter: u32,
    messages: Vec<String>,
}

struct Component;

impl Guest for Component {
    fn init(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} actor");
        let (self_id,) = params;
        log(&format!("Actor ID: {}", &self_id));
        log("Hello from {{project_name}} actor!");

        // Parse existing state or create new
        let actor_state = match state {
            Some(bytes) => {
                serde_json::from_slice::<ActorState>(&bytes)
                    .unwrap_or_else(|_| ActorState::default())
            }
            None => ActorState::default(),
        };

        // Serialize state back
        let new_state = serde_json::to_vec(&actor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        // For demo, we'll shutdown after init - remove this for persistent actors
        shutdown(None);

        Ok((Some(new_state),))
    }
}

bindings::export!(Component with_types_in bindings);
