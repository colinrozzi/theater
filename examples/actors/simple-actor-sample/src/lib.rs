mod bindings;

use bindings::exports::ntwk::simple_actor::actor::Guest;
use bindings::exports::ntwk::simple_actor::actor::Message;
use bindings::exports::ntwk::simple_actor::actor::State;

use bindings::ntwk::simple_actor::runtime::log;

struct Component;

fn parse_json(data: &[u8]) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let str_data = String::from_utf8(data.to_vec())?;
    let json = serde_json::from_str(&str_data)?;
    Ok(json)
}

fn get_counter(state_json: &serde_json::Value) -> Result<u64, Box<dyn std::error::Error>> {
    let counter_str = state_json
        .get("counter")
        .and_then(|v| v.as_str())
        .ok_or("counter not found or not a string")?;
    let counter = counter_str.parse::<u64>()?;
    Ok(counter)
}

impl Guest for Component {
    fn init() -> Vec<u8> {
        log(&"Hello from actor4!".to_string());
        let state = b"{\"counter\": \"0\"}".to_vec();
        state
    }

    fn handle(msg: Message, state: State) -> State {
        log(&"Processing message in actor4".to_string());
        let mut new_state = state.clone();

        // Log received message
        if let Ok(msg_str) = String::from_utf8(msg.clone()) {
            log(&format!("Received message: {}", msg_str));
        }

        // Process message
        let result = parse_json(&msg).and_then(|msg_json| {
            if msg_json.get("action").and_then(|a| a.as_str()) == Some("increment") {
                let state_json = parse_json(&new_state)?;
                let counter = get_counter(&state_json)?;

                let mut updated_json = state_json;
                updated_json["counter"] = serde_json::Value::String((counter + 1).to_string());
                new_state = serde_json::to_vec(&updated_json)?;
            }
            Ok(())
        });

        if let Err(e) = result {
            log(&format!("Error processing message: {}", e));
        }

        new_state
    }

    fn state_contract(state: State) -> bool {
        if let Ok(state_str) = String::from_utf8(state.clone()) {
            // Verify the state is valid JSON
            serde_json::from_str::<serde_json::Value>(&state_str).is_ok()
        } else {
            false
        }
    }

    fn message_contract(msg: Message, _state: State) -> bool {
        if let Ok(msg_str) = String::from_utf8(msg.clone()) {
            // Verify the message is valid JSON
            serde_json::from_str::<serde_json::Value>(&msg_str).is_ok()
        } else {
            false
        }
    }
}

bindings::export!(Component with_types_in bindings);
