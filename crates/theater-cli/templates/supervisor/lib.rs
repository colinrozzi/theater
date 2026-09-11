#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::exports::theater::simple::lifecycle_handlers::Guest as LifecycleHandlers;
use bindings::theater::simple::self_::log;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct SupervisorState {
    children: Vec<String>,
}

struct Component;

impl Guest for Component {
    fn init(state: Option<Vec<u8>>, params: (String,)) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} supervisor actor");
        let (self_id,) = params;
        log(&format!("Supervisor ID: {}", &self_id));

        // Actor management is now a runtime primitive: spawn/stop/inspect any
        // actor by id via `theater:simple/runtime`, and attach a monitor via
        // `theater:simple/lifecycle` so a child's terminal event is delivered to
        // `handle-lifecycle-event` below. (There is no separate supervisor
        // interface anymore, and spawn no longer auto-monitors — attach the
        // monitor explicitly after spawning.)

        let supervisor_state = match state {
            Some(bytes) => {
                serde_json::from_slice::<SupervisorState>(&bytes).unwrap_or_default()
            }
            None => SupervisorState::default(),
        };

        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        Ok((Some(new_state),))
    }
}

impl LifecycleHandlers for Component {
    // The single death/lifecycle callback (it replaced the old
    // error/exit/external-stop trio). Fires for every actor this one monitors.
    // `subject` is the monitored actor's id; `data` is the pack-encoded
    // lifecycle event payload (decode for the cause + final state).
    fn handle_lifecycle_event(
        subject: String,
        event_type: String,
        _data: Vec<u8>,
    ) -> Result<(), String> {
        log(&format!(
            "Lifecycle event from {}: {}",
            subject, event_type
        ));
        Ok(())
    }
}

bindings::export!(Component with_types_in bindings);
