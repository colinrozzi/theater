use thiserror::Error;
use tracing::info;

use crate::store::{ContentRef, ContentStore};

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
