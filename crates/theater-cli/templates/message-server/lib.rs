#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::exports::theater::simple::message_server_client::Guest as MessageServerClient;
use bindings::theater::simple::runtime::log;
use bindings::theater::simple::types::{ChannelAccept, ChannelId};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct ActorState {
    messages: Vec<String>,
    channels: Vec<String>,
}

struct Component;

impl Guest for Component {
    fn init(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} message server actor");
        let (self_id,) = params;
        log(&format!("Actor ID: {}", &self_id));

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

        Ok((Some(new_state),))
    }
}

impl MessageServerClient for Component {
    fn handle_send(
        state: Option<Vec<u8>>,
        params: (Vec<u8>,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (data,) = params;
        log(&format!("Received message: {} bytes", data.len()));
        
        // Parse and update state
        let mut actor_state: ActorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => ActorState::default(),
        };
        
        // Store message (as string if possible)
        if let Ok(msg) = String::from_utf8(data) {
            actor_state.messages.push(msg);
        }
        
        let new_state = serde_json::to_vec(&actor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        Ok((Some(new_state),))
    }

    fn handle_request(
        state: Option<Vec<u8>>,
        params: (String, Vec<u8>),
    ) -> Result<(Option<Vec<u8>>, (Option<Vec<u8>>,)), String> {
        let (request_id, data) = params;
        log(&format!("Received request {}: {} bytes", request_id, data.len()));
        
        // Echo the data back as response
        Ok((state, (Some(data),)))
    }

    fn handle_channel_open(
        state: Option<Vec<u8>>,
        params: (String, Vec<u8>),
    ) -> Result<(Option<Vec<u8>>, (ChannelAccept,)), String> {
        let (channel_id, _data) = params;
        log(&format!("Channel open request: {}", channel_id));
        
        // Accept all channel requests
        Ok((
            state,
            (ChannelAccept {
                accepted: true,
                message: Some(b"Welcome to the channel!".to_vec()),
            },),
        ))
    }

    fn handle_channel_close(
        state: Option<Vec<u8>>,
        params: (ChannelId,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (channel_id,) = params;
        log(&format!("Channel closed: {}", channel_id));
        Ok((state,))
    }

    fn handle_channel_message(
        state: Option<Vec<u8>>,
        params: (ChannelId, Vec<u8>),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (channel_id, data) = params;
        log(&format!("Channel {} message: {} bytes", channel_id, data.len()));
        Ok((state,))
    }
}

bindings::export!(Component with_types_in bindings);
