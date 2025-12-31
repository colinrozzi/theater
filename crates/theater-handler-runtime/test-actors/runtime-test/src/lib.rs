mod bindings;

use bindings::theater::simple::runtime;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(
        _state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (actor_id,) = params;
        
        // Test 1: Log a message
        runtime::log(&format!("Runtime test actor initialized with ID: {}", actor_id));
        
        // Test 2: Log multiple messages
        runtime::log("Test message 1");
        runtime::log("Test message 2");
        runtime::log("Test message 3");
        
        // Test 3: Get the event chain
        let chain = runtime::get_chain();
        let event_count = chain.events.len();
        runtime::log(&format!("Event chain has {} events", event_count));
        
        // Verify we have some events (at least the init call and our logs)
        if event_count == 0 {
            return Err("Expected event chain to have events".to_string());
        }
        
        // Test 4: Log event types we found
        for (i, meta_event) in chain.events.iter().enumerate() {
            runtime::log(&format!(
                "Event {}: type='{}', hash={}",
                i,
                meta_event.event.event_type,
                meta_event.hash
            ));
        }
        
        // Return success state
        let result = serde_json::json!({
            "status": "success",
            "actor_id": actor_id,
            "event_count": event_count,
            "message": "Runtime handler tests passed!"
        });
        
        Ok((Some(serde_json::to_vec(&result).unwrap()),))
    }
}

bindings::export!(Component with_types_in bindings);
