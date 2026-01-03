mod bindings;

use bindings::theater::simple::runtime;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(_state: Option<Vec<u8>>) -> Result<(Option<Vec<u8>>,), String> {
        runtime::log("Runtime test actor initialized");

        runtime::log("Test message 1");
        runtime::log("Test message 2");
        runtime::log("Test message 3");

        // Return success state
        let result = serde_json::json!({
            "status": "success",
            "message": "Runtime handler tests passed!"
        });

        Ok((Some(serde_json::to_vec(&result).unwrap()),))
    }
}

bindings::export!(Component with_types_in bindings);
