mod bindings;

use crate::bindings::exports::ntwk::theater::actor::Guest;
use crate::bindings::ntwk::theater::runtime::{log, shutdown};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct State {
    messages: Vec<String>,
}

struct Actor;
impl Guest for Actor {
    fn init(
        _init_state_bytes: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} actor");
        let (self_id,) = params;
        log(&format!("Actor ID: {}", &self_id));
        log("Hello from {{project_name}} actor!");
        shutdown(None);

        Ok((None,))
    }
}

bindings::export!(Actor with_types_in bindings);
