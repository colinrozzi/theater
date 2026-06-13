use serde_json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::info;

use crate::store::{ContentRef, ContentStore, Label};
use anyhow::Result;

// Add the template module
pub mod template;

#[derive(Error, Debug)]
pub enum ReferenceError {
    #[error("Failed to resolve reference: {0}")]
    ResolveError(String),
}

/// In-memory cache of fetched-resource bytes, keyed by reference string.
///
/// Sized for the spawn-fetch use case: a parent actor spawning the same
/// child wasm repeatedly. Each entry is the raw bytes of one resource
/// (manifest, wasm package, etc.); typical entry is a few MB.
///
/// The cache is opt-in at the call site (via [`resolve_reference_cached`])
/// and at the manifest level (via the `static_package` field on
/// [`crate::config::actor_manifest::ManifestConfig`]). The default
/// behavior of `theater spawn` and `supervisor.spawn` is uncached —
/// every spawn fetches. Opt-in means the operator is asserting the URL
/// is content-addressed for the lifetime of the theater process.
///
/// Entries live for the lifetime of the cache; redeploying an actor at
/// a mutable URL would require restarting theater (the same model the
/// VPS topology runs today). Cache size is unbounded; for prod's
/// stable actor set this is fine — same shape as the compiled-module
/// cache in [`crate::pack_bridge::CachingPackRuntime`].
///
/// What the cache actually saves: the network round-trip on every spawn
/// after the first. The cache holds one Arc-shared copy of each
/// resource's bytes, but the supervisor's spawn pipeline currently
/// copies the bytes out of the Arc to satisfy a `Vec<u8>`-by-value
/// contract downstream — so steady-state RAM is `1 × cached + N × in
/// flight per spawn`, not `1 × cached shared with N spawns`. A
/// follow-up that threads `Arc<Vec<u8>>` down into the runtime command
/// would close that gap; the bytewise copy is microseconds versus
/// ~10 ms cranelift, so not on the critical path today.
///
/// The lock is `std::sync::RwLock` because critical sections are
/// HashMap get/insert only and never held across `.await`.
#[derive(Default)]
pub struct ResourceCache {
    entries: RwLock<HashMap<String, Arc<Vec<u8>>>>,
}

impl ResourceCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct references currently cached.
    pub fn len(&self) -> usize {
        self.entries
            .read()
            .expect("resource cache lock poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Like [`resolve_reference`], but checks `cache` first and populates it
/// on miss. Returns the bytes (shared via `Arc` so cache hits don't
/// copy) and whether the result came from the cache.
///
/// Concurrent misses on the same reference both fetch; the last insert
/// wins. Benign — both copies are valid — and preferable to holding the
/// write lock across the network round-trip.
pub async fn resolve_reference_cached(
    reference: &str,
    cache: &ResourceCache,
) -> Result<(Arc<Vec<u8>>, bool), ReferenceError> {
    if let Some(hit) = cache
        .entries
        .read()
        .expect("resource cache lock poisoned")
        .get(reference)
        .cloned()
    {
        return Ok((hit, true));
    }

    let bytes = resolve_reference(reference).await?;
    let arc = Arc::new(bytes);
    cache
        .entries
        .write()
        .expect("resource cache lock poisoned")
        .insert(reference.to_string(), arc.clone());
    Ok((arc, false))
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
            Err(ReferenceError::ResolveError(format!(
                "Invalid store path format: {}",
                reference
            )))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn resource_cache_hits_on_repeat_reference() {
        // Write a small temp file the cache can resolve against; using a
        // file:// reference here keeps this test offline.
        let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
        tmp.write_all(b"hello cache").expect("write");
        let path = tmp.path().to_string_lossy().to_string();

        let cache = ResourceCache::new();
        assert_eq!(cache.len(), 0);

        let (bytes, hit) = resolve_reference_cached(&path, &cache).await.unwrap();
        assert_eq!(&**bytes, b"hello cache");
        assert!(!hit, "first fetch should be a miss");
        assert_eq!(cache.len(), 1);

        let (bytes2, hit2) = resolve_reference_cached(&path, &cache).await.unwrap();
        assert_eq!(&**bytes2, b"hello cache");
        assert!(hit2, "second fetch should be a hit");
        assert_eq!(cache.len(), 1);

        // Same Arc payload — cache hands out clones of the same Arc.
        assert!(Arc::ptr_eq(&bytes, &bytes2));
    }

    /// Two tasks racing on the same uncached reference both fetch
    /// (because both observe a miss before either inserts) and both
    /// get the same byte content back. Locks in the "concurrent
    /// misses both compile, last insert wins, benign" contract
    /// documented on [`ResourceCache`].
    #[tokio::test]
    async fn resource_cache_concurrent_misses_both_fetch_safely() {
        let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
        tmp.write_all(b"raced bytes").expect("write");
        let path = tmp.path().to_string_lossy().to_string();

        let cache = Arc::new(ResourceCache::new());
        let path_a = path.clone();
        let path_b = path.clone();
        let cache_a = cache.clone();
        let cache_b = cache.clone();

        let (a, b) = tokio::join!(
            tokio::spawn(async move { resolve_reference_cached(&path_a, &cache_a).await }),
            tokio::spawn(async move { resolve_reference_cached(&path_b, &cache_b).await }),
        );

        let (bytes_a, _) = a.unwrap().unwrap();
        let (bytes_b, _) = b.unwrap().unwrap();
        assert_eq!(&**bytes_a, b"raced bytes");
        assert_eq!(&**bytes_b, b"raced bytes");
        // One entry in the cache regardless of which task's insert won.
        assert_eq!(cache.len(), 1);
    }

    #[tokio::test]
    async fn resource_cache_keys_by_reference_string() {
        let mut tmp_a = tempfile::NamedTempFile::new().expect("a");
        tmp_a.write_all(b"A").expect("write");
        let mut tmp_b = tempfile::NamedTempFile::new().expect("b");
        tmp_b.write_all(b"B").expect("write");

        let cache = ResourceCache::new();
        let (a, _) = resolve_reference_cached(&tmp_a.path().to_string_lossy(), &cache)
            .await
            .unwrap();
        let (b, _) = resolve_reference_cached(&tmp_b.path().to_string_lossy(), &cache)
            .await
            .unwrap();

        assert_eq!(&**a, b"A");
        assert_eq!(&**b, b"B");
        assert_eq!(
            cache.len(),
            2,
            "different references must be distinct entries"
        );
    }
}
