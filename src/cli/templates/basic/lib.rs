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
    fn init(_state: State, params: (String,)) -> Result<(State,), String> {
        log("Initializing {{project_name}} actor");
        let (param,) = params;
        log(&format!("Init parameter: {}", param));

        let app_state = AppState::default();
        log("Created default app state");
        let state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;

        // Create the initial state
        let new_state = Some(state_bytes);

        Ok((new_state,))
    }
}

impl MessageServerClient for Actor {
    fn handle_send(
        state: Option<Vec<u8>>,
        params: (Vec<u8>,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Handling send message");
        let (data,) = params;

        // Parse the current state
        let state_bytes = state.unwrap_or_default();
        let mut app_state: AppState = if !state_bytes.is_empty() {
            serde_json::from_slice(&state_bytes).map_err(|e| e.to_string())?
        } else {
            AppState::default()
        };

        // Try to parse the message as a string
        if let Ok(message) = String::from_utf8(data) {
            log(&format!("Received message: {}", message));
            app_state.messages.push(message);
        } else {
            log("Received binary data");
            app_state.count += 1;
        }

        // Save the updated state
        let updated_state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;
        let updated_state = Some(updated_state_bytes);

        Ok((updated_state,))
    }

    fn handle_request(
        state: Option<Vec<u8>>,
        params: (Vec<u8>,),
    ) -> Result<(Option<Vec<u8>>, (Vec<u8>,)), String> {
        log("Handling request message");
        let (data,) = params;

        // Parse the current state
        let state_bytes = state.unwrap_or_default();
        let mut app_state: AppState = if !state_bytes.is_empty() {
            serde_json::from_slice(&state_bytes).map_err(|e| e.to_string())?
        } else {
            AppState::default()
        };

        // Try to parse the message as a string
        let response = if let Ok(message) = String::from_utf8(data.clone()) {
            log(&format!("Received request: {}", message));

            match message.as_str() {
                "count" => {
                    let response = format!("Current count: {}", app_state.count);
                    response.into_bytes()
                }
                "messages" => {
                    let response = format!("Messages: {:?}", app_state.messages);
                    response.into_bytes()
                }
                "increment" => {
                    app_state.count += 1;
                    let response = format!("Count incremented to: {}", app_state.count);
                    response.into_bytes()
                }
                _ => {
                    // Store the message
                    app_state.messages.push(message);
                    let response = "Message stored".to_string();
                    response.into_bytes()
                }
            }
        } else {
            log("Received binary data request");
            // Just echo back the data
            data
        };

        // Save the updated state
        let updated_state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;
        let updated_state = Some(updated_state_bytes);

        Ok((updated_state, (response,)))
    }

    fn handle_channel_open(
        state: Option<bindings::exports::ntwk::theater::message_server_client::Json>,
        params: (bindings::exports::ntwk::theater::message_server_client::Json,),
    ) -> Result<
        (
            Option<bindings::exports::ntwk::theater::message_server_client::Json>,
            (bindings::exports::ntwk::theater::message_server_client::ChannelAccept,),
        ),
        String,
    > {
        Ok((
            state,
            (
                bindings::exports::ntwk::theater::message_server_client::ChannelAccept {
                    accepted: true,
                    message: None,
                },
            ),
        ))
    }

    fn handle_channel_close(
        state: Option<bindings::exports::ntwk::theater::message_server_client::Json>,
        params: (String,),
    ) -> Result<(Option<bindings::exports::ntwk::theater::message_server_client::Json>,), String>
    {
        Ok((state,))
    }

    fn handle_channel_message(
        state: Option<bindings::exports::ntwk::theater::message_server_client::Json>,
        params: (
            String,
            bindings::exports::ntwk::theater::message_server_client::Json,
        ),
    ) -> Result<(Option<bindings::exports::ntwk::theater::message_server_client::Json>,), String>
    {
        log("runtime-content-fs: Received channel message");
        Ok((state,))
    }
}

bindings::export!(Actor with_types_in bindings);
