use serde::{Deserialize, Serialize};
use serde_json;

mod bindings;

use crate::bindings::ntwk::simple_http_actor::types::Message;
use bindings::exports::ntwk::simple_http_actor::http_actor::Guest;
use bindings::exports::ntwk::simple_http_actor::http_actor::HttpRequest;
use bindings::exports::ntwk::simple_http_actor::http_actor::State;
use bindings::ntwk::simple_http_actor::http_runtime::log;

struct Component;

#[derive(Serialize, Deserialize)]
struct ActorState {
    content: String,
}

impl Guest for Component {
    fn init() -> Vec<u8> {
        log("Initializing frontend actor");
        // Initial state contains our HTML content
        serde_json::to_vec(&serde_json::json!({
            "content": r#"<!DOCTYPE html>
<html>
<head>
    <title>Simple Frontend</title>
</head>
<body>
    <h1>Hello from the Frontend Actor!</h1>
    <p>This is a simple HTML page served by our WebAssembly actor.</p>
</body>
</html>"#
        }))
        .unwrap()
    }

    fn http_contract(req: HttpRequest, state: State) -> bool {
        // Accept any path for now
        true
    }

    fn handle_http(req: HttpRequest, state: State) -> Vec<u8> {
        log("Received HTTP request");
        let parsed_state: ActorState = serde_json::from_slice(&state).unwrap();
        log(&format!("State: {}", parsed_state.content));
        // For now, we just return the current state (our HTML content)
        // we will return a JSON object with a http request and state
        /*
        {
            "response": {
                "status": 200,
                "headers": {
                    "Content-Type": "text/html"
                },
                "body": state
            },
            "state": state
        }
        */
        let response = serde_json::json!({
            "response": {
            "status": 200,
            "headers": {
                "Content-Type": "text/html"
            },
            "body": parsed_state.content
        },
        "state": parsed_state
        });

        log(&format!("Response: {}", response));

        let response_bytes = serde_json::to_vec(&response).unwrap();
        response_bytes
    }

    fn handle(msg: Message, state: State) -> State {
        log("Received request");
        // For now, we just return the current state (our HTML content)
        state
    }

    fn message_contract(msg: Message, _state: State) -> bool {
        // Accept any valid UTF-8 message for now
        String::from_utf8(msg).is_ok()
    }

    fn state_contract(state: State) -> bool {
        // Verify state is valid JSON and has our content field
        if let Ok(state_str) = String::from_utf8(state.clone()) {
            if let Ok(state_json) = serde_json::from_str::<serde_json::Value>(&state_str) {
                state_json.get("content").is_some()
            } else {
                false
            }
        } else {
            false
        }
    }
}

bindings::export!(Component with_types_in bindings);
