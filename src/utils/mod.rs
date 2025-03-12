use thiserror::Error;
use tracing::info;
use serde_json;

use crate::store::{ContentRef, ContentStore};
use anyhow::Result;

#[derive(Error, Debug)]
pub enum ReferenceError {
    #[error("Failed to resolve reference: {0}")]
    ResolveError(String),
}

/// Resolve a reference to a byte array.
/// A reference can be a file path, a store path, or in the future a URL.
/// The reference is resolved to a byte array.
///
/// a store path is a reference to a file in the store and is prefixed with `store://`
/// a store path can be either a hash or a path to a label.
/// store://<store-id>/hash/<hash>
/// store://<store-id>/<label>
///
/// everything else is treated as a file path.
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
        if parts.len() == 4 && parts[3] == "hash" {
            // store path with hash
            let hash = parts[4];
            let content_ref = ContentRef::from_str(hash);
            info!("Resolving store path with hash: {}", hash);
            store
                .get(&content_ref)
                .await
                .map_err(|e| ReferenceError::ResolveError(e.to_string()))
        } else {
            // store path with label
            let label = parts[3];
            info!("Resolving store path with label: {}", label);
            match store.get_content_by_label(label).await {
                Ok(result) => match result {
                    Some(content) => Ok(content),
                    None => Err(ReferenceError::ResolveError(format!(
                        "Label not found: {}",
                        label
                    ))),
                },
                Err(e) => Err(ReferenceError::ResolveError(e.to_string())),
            }
        }
    } else {
        // file path
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
pub fn merge_initial_states(config_state: Option<Vec<u8>>, override_state: Option<Vec<u8>>) -> Result<Option<Vec<u8>>> {
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
                    if let (serde_json::Value::Object(ref mut config_map), serde_json::Value::Object(override_map)) = 
                        (&mut config_json, &override_json) {
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
                },
                _ => {
                    // If either parsing fails, just use the override
                    info!("Failed to parse one of the initial states as JSON, using override state");
                    Ok(Some(override_state))
                }
            }
        }
    }
}
