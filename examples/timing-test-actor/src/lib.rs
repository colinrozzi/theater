wit_bindgen::generate!({
    world: "default",
    path: "wit",
});

use exports::theater::simple::actor::Guest;
use theater::simple::runtime::{log, shutdown};
use theater::simple::timing::{now, sleep};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct ActorState {
    timestamps: Vec<u64>,
}

struct Component;

impl Guest for Component {
    fn init(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("=== Timing Test Actor Starting ===");
        let (self_id,) = params;
        log(&format!("Actor ID: {}", &self_id));

        // Parse existing state or create new
        let mut actor_state = match state {
            Some(bytes) => {
                serde_json::from_slice::<ActorState>(&bytes)
                    .unwrap_or_else(|_| ActorState::default())
            }
            None => ActorState::default(),
        };

        // Test 1: Get current time
        let t1 = now();
        log(&format!("Test 1 - now(): {}", t1));
        actor_state.timestamps.push(t1);

        // Test 2: Get time again (should be slightly later)
        let t2 = now();
        log(&format!("Test 2 - now(): {} (delta: {}ms)", t2, t2 - t1));
        actor_state.timestamps.push(t2);

        // Test 3: Sleep for 10ms
        log("Test 3 - sleeping for 10ms...");
        match sleep(10) {
            Ok(()) => log("Test 3 - sleep completed successfully"),
            Err(e) => log(&format!("Test 3 - sleep failed: {}", e)),
        }

        // Test 4: Get time after sleep
        let t3 = now();
        log(&format!("Test 4 - now() after sleep: {} (delta from t2: {}ms)", t3, t3 - t2));
        actor_state.timestamps.push(t3);

        // Test 5: Another now() call
        let t4 = now();
        log(&format!("Test 5 - final now(): {}", t4));
        actor_state.timestamps.push(t4);

        log("=== Timing Test Actor Complete ===");
        log(&format!("Total timestamps recorded: {}", actor_state.timestamps.len()));

        // Serialize state back
        let new_state = serde_json::to_vec(&actor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        // Shutdown after test
        shutdown(None);

        Ok((Some(new_state),))
    }
}

export!(Component);
