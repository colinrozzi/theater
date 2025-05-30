use serde_json;
use thiserror::Error;
use tracing::{debug, info};

use crate::store::{ContentRef, ContentStore, Label};
use crate::{ChainEvent, TheaterId};
use anyhow::{anyhow, Result};

#[derive(Error, Debug)]
pub enum ReferenceError {
    #[error("Failed to resolve reference: {0}")]
    ResolveError(String),
}

/// Resolve a reference to a byte array.
/// A reference can be a file path, a store path, or a URL.
/// The reference is resolved to a byte array.
///
/// A store path is a reference to a file in the store and is prefixed with `store://`
/// A store path can be either a hash or a path to a label:
/// - store://<store-id>/hash/<hash>
/// - store://<store-id>/<label>
///
/// A URL is any reference starting with `http://` or `https://`
///
/// Everything else is treated as a file path.
pub async fn resolve_reference(reference: &str) -> Result<Vec<u8>, ReferenceError> {
    info!("Resolving reference: {}", reference);

    if reference.starts_with("store://") {
        // store path
        let parts: Vec<&str> = reference.split('/').collect();
        if parts.len() < 3 {
            return Err(ReferenceError::ResolveError(format!(
                "Invalid store path: {}",
                reference
            )));
        }
        let store_id = parts[2];
        let store = ContentStore::from_id(store_id);

        if parts.len() >= 5 && parts[3] == "hash" {
            // store path with hash
            let hash = parts[4];
            let content_ref = ContentRef::from_str(hash);
            info!("Resolving store path with hash: {}", hash);
            store
                .get(&content_ref)
                .await
                .map_err(|e| ReferenceError::ResolveError(e.to_string()))
        } else if parts.len() >= 4 {
            // store path with label
            let label_string = parts[3];
            let label = Label::new(label_string.to_string());
            info!("Resolving store path with label: {}", label);
            match store.get_content_by_label(&label).await {
                Ok(result) => match result {
                    Some(content) => Ok(content),
                    None => Err(ReferenceError::ResolveError(format!(
                        "Label not found: {}",
                        label
                    ))),
                },
                Err(e) => Err(ReferenceError::ResolveError(e.to_string())),
            }
        } else {
            return Err(ReferenceError::ResolveError(format!(
                "Invalid store path format: {}",
                reference
            )));
        }
    } else if reference.starts_with("http://") || reference.starts_with("https://") {
        // HTTP/HTTPS URL
        info!("Fetching from URL: {}", reference);
        let client = reqwest::Client::new();
        match client.get(reference).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.bytes().await {
                        Ok(bytes) => Ok(bytes.to_vec()),
                        Err(e) => Err(ReferenceError::ResolveError(format!(
                            "Failed to read response body from {}: {}",
                            reference, e
                        ))),
                    }
                } else {
                    Err(ReferenceError::ResolveError(format!(
                        "HTTP request failed for {}: {} {}",
                        reference,
                        response.status().as_u16(),
                        response
                            .status()
                            .canonical_reason()
                            .unwrap_or("Unknown error")
                    )))
                }
            }
            Err(e) => Err(ReferenceError::ResolveError(format!(
                "Failed to fetch from {}: {}",
                reference, e
            ))),
        }
    } else {
        // file path
        info!("Reading from file path: {}", reference);
        tokio::fs::read(reference)
            .await
            .map_err(|e| ReferenceError::ResolveError(e.to_string()))
    }
}

/// Merge two optional initial states, with the override state taking precedence
/// for any overlapping keys.
///
/// The states should be JSON objects. If either state is not a valid JSON object,
/// the following rules apply:
/// - If config_state is None and override_state is None, returns None
/// - If config_state is Some and override_state is None, returns config_state
/// - If config_state is None and override_state is Some, returns override_state
/// - If both are Some but either is not a valid JSON object, returns override_state
/// - If both are Some and both are valid JSON objects, merges them with override_state taking precedence
pub fn merge_initial_states(
    config_state: Option<Vec<u8>>,
    override_state: Option<Vec<u8>>,
) -> Result<Option<Vec<u8>>> {
    match (config_state, override_state) {
        (None, None) => Ok(None),
        (Some(state), None) => Ok(Some(state)),
        (None, Some(state)) => Ok(Some(state)),
        (Some(config_state), Some(override_state)) => {
            // Parse both states
            let config_json_result = serde_json::from_slice(&config_state);
            let override_json_result = serde_json::from_slice(&override_state);

            match (config_json_result, override_json_result) {
                (Ok(mut config_json), Ok(override_json)) => {
                    // Ensure both are objects
                    if let (
                        serde_json::Value::Object(ref mut config_map),
                        serde_json::Value::Object(override_map),
                    ) = (&mut config_json, &override_json)
                    {
                        // Merge override values into config
                        for (key, value) in override_map {
                            config_map.insert(key.clone(), value.clone());
                        }

                        // Serialize the merged result
                        Ok(Some(serde_json::to_vec(&config_json)?))
                    } else {
                        // If either isn't an object, just use the override
                        info!("Either initial state is not a JSON object, using override state");
                        Ok(Some(override_state))
                    }
                }
                _ => {
                    // If either parsing fails, just use the override
                    info!(
                        "Failed to parse one of the initial states as JSON, using override state"
                    );
                    Ok(Some(override_state))
                }
            }
        }
    }
}

pub fn get_theater_home() -> String {
    std::env::var("THEATER_HOME").unwrap_or_else(|_| {
        format!(
            "{}/{}",
            std::env::var("HOME").unwrap_or_default(),
            ".theater"
        )
    })
}

/// Read events from filesystem for an actor that isn't currently running
pub fn read_events_from_filesystem(actor_id: &TheaterId) -> Result<Vec<ChainEvent>> {
    // Determine the Theater home directory
    let theater_home = get_theater_home();

    let chains_dir = format!("{}/chains", theater_home);
    let events_dir = format!("{}/events", theater_home);

    // Check if the actor's chain file exists
    let chain_path = format!("{}/{}", chains_dir, actor_id);
    if !std::path::Path::new(&chain_path).exists() {
        debug!("No chain file found at: {}", chain_path);
        return Err(anyhow!("No stored events found for actor: {}", actor_id));
    }

    // Read the chain head hash
    let head_data = std::fs::read_to_string(&chain_path)?;
    let head_hash: Option<Vec<u8>> = serde_json::from_str(&head_data)?;

    if head_hash.is_none() {
        debug!("Empty chain head for actor: {}", actor_id);
        return Ok(Vec::new()); // Empty chain
    }

    // Reconstruct the full chain by following parent hash links
    let mut events = Vec::new();
    let mut current_hash = head_hash;

    while let Some(hash) = current_hash {
        let hash_hex = hex::encode(&hash);
        let event_path = format!("{}/{}", events_dir, hash_hex);

        // Read and parse the event
        let event_data = match std::fs::read_to_string(&event_path) {
            Ok(data) => data,
            Err(e) => {
                debug!("Failed to read event file {}: {}", event_path, e);
                break; // Break the chain if we can't read an event file
            }
        };

        let event = match serde_json::from_str::<ChainEvent>(&event_data) {
            Ok(event) => event,
            Err(e) => {
                debug!("Failed to parse event from {}: {}", event_path, e);
                break; // Break the chain if we can't parse an event
            }
        };

        // Store the event and move to the parent
        current_hash = event.parent_hash.clone();
        events.push(event);
    }

    // Reverse the events to get them in chronological order (oldest first)
    events.reverse();

    debug!(
        "Read {} events from filesystem for actor {}",
        events.len(),
        actor_id
    );
    Ok(events)
}
