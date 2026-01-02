#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::theater::simple::runtime::{log, shutdown};
use bindings::theater::simple::timing::{now, sleep};

struct Component;

impl Guest for Component {
    fn init(_state: Option<Vec<u8>>, params: (String,)) -> Result<(Option<Vec<u8>>,), String> {
        let (actor_id,) = params;
        log(&format!("=== Simple Timing Test Actor Starting ==="));
        log(&format!("Actor ID: {}", actor_id));

        // Test 1: Get current time
        let t1 = now();
        log(&format!("Test 1 - now(): {}", t1));

        // Test 2: Get time again (should be same or slightly later)
        let t2 = now();
        log(&format!("Test 2 - now(): {} (delta: {}ms)", t2, t2.saturating_sub(t1)));

        // Test 3: Sleep for 10ms
        log("Test 3 - sleeping for 10ms...");
        match sleep(10) {
            Ok(()) => log("Test 3 - sleep completed successfully"),
            Err(e) => log(&format!("Test 3 - sleep failed: {}", e)),
        }

        // Test 4: Get time after sleep
        let t3 = now();
        log(&format!("Test 4 - now() after sleep: {} (delta from t2: {}ms)", t3, t3.saturating_sub(t2)));

        // Test 5: Another now() call
        let t4 = now();
        log(&format!("Test 5 - final now(): {}", t4));

        log("=== Simple Timing Test Actor Complete ===");

        // Shutdown
        shutdown(None);

        Ok((Some(b"Timing tests completed".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
