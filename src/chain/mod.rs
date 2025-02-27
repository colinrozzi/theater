use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fmt;
use std::path::Path;
use std::sync::mpsc;
use wasmtime::component::{ComponentType, Lift, Lower};

use crate::events::filesystem::FilesystemEvent;
use crate::events::http::HttpEvent;
use crate::events::message::MessageEvent;
use crate::events::runtime::RuntimeEvent;
use crate::events::supervisor::SupervisorEvent;
use crate::events::ChainEventData;
use crate::store::ContentStore;
use crate::TheaterId;

#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct ChainEvent {
    pub hash: Vec<u8>,
    #[component(name = "parent-hash")]
    pub parent_hash: Option<Vec<u8>>,
    #[component(name = "event-type")]
    pub event_type: String,
    pub data: Vec<u8>,
    pub timestamp: u64,
    // Optional human-readable description
    pub description: Option<String>,
}

impl ChainEvent {
    /// Create a new event from a typed event data object
    pub fn new<T: ChainEventData>(
        event_data: T,
        parent_hash: Option<Vec<u8>>,
    ) -> Result<Self, serde_json::Error> {
        let data = event_data.to_json()?;
        let event_type = event_data.event_type().to_string();
        let description = Some(event_data.description());

        // Hash calculation will be done in add_event
        // This is just a placeholder
        let hash = Vec::new();

        Ok(Self {
            hash,
            parent_hash,
            event_type,
            data,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            description,
        })
    }

    /// Try to parse the event data as a specific event type
    pub fn parse_data<T: ChainEventData>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.data)
    }
}

impl fmt::Display for ChainEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format timestamp as human-readable date with millisecond precision
        let datetime = chrono::DateTime::from_timestamp(self.timestamp as i64, 0)
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
        let formatted_time = datetime.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        // Format hash as short hex string (first 7 characters)
        let hash_str = self
            .hash
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let short_hash = if hash_str.len() > 7 {
            &hash_str[0..7]
        } else {
            &hash_str
        };

        // Format parent hash if it exists
        let parent_str = match &self.parent_hash {
            Some(ph) => {
                let ph_str = ph.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                if ph_str.len() > 7 {
                    format!("(parent: {}...)", &ph_str[0..7])
                } else {
                    format!("(parent: {})", ph_str)
                }
            }
            None => "(root)".to_string(),
        };

        // Use the description if available
        let content = if let Some(desc) = &self.description {
            desc.clone()
        } else {
            // Format data preview, attempting JSON formatting if possible
            if let Ok(text) = std::str::from_utf8(&self.data) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                    if json.is_object() && json.as_object().unwrap().len() <= 3 {
                        // For small JSON objects, inline them
                        serde_json::to_string(&json).unwrap_or_else(|_| text.to_string())
                    } else {
                        // For larger JSON, just show a preview
                        let preview = if text.len() > 30 {
                            format!("{}...", &text[0..27])
                        } else {
                            text.to_string()
                        };
                        format!("'{}'", preview)
                    }
                } else {
                    // Not JSON, just show text preview
                    let preview = if text.len() > 30 {
                        format!("{}...", &text[0..27])
                    } else {
                        text.to_string()
                    };
                    format!("'{}'", preview)
                }
            } else {
                // Binary data
                format!("{} bytes of binary data", self.data.len())
            }
        };

        write!(
            f,
            "[{}] Event[{}] {} {} {}",
            formatted_time,
            short_hash,
            parent_str,
            style(&self.event_type).cyan(),
            content
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChain {
    events: Vec<ChainEvent>,
    current_hash: Option<Vec<u8>>,
    #[serde(skip)]
    event_callback: Option<mpsc::Sender<ChainEvent>>,
    #[serde(skip)]
    content_store: ContentStore,
    actor_id: TheaterId,
}

impl StateChain {
    pub fn new(actor_id: TheaterId, content_store: ContentStore) -> Self {
        Self {
            events: Vec::new(),
            current_hash: None,
            event_callback: None,
            content_store,
            actor_id,
        }
    }

    /// Add a typed event to the chain
    pub async fn add_typed_event<T: ChainEventData>(
        &mut self,
        event_data: T,
    ) -> Result<ChainEvent, serde_json::Error> {
        // Create initial event structure without hash
        let mut event = ChainEvent {
            hash: Vec::new(), // Will be set after storage
            parent_hash: self.current_hash.clone(),
            event_type: event_data.event_type().to_string(),
            data: event_data.to_json()?,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            description: Some(event_data.description()),
        };

        // Store the event data in the content store
        let serialized_event = serde_json::to_vec(&event)?;
        let content_ref = self.content_store.store(serialized_event).await.unwrap();

        // Get the hash from ContentRef and use it as the event hash
        let hash_bytes = hex::decode(content_ref.hash()).unwrap();
        event.hash = hash_bytes.clone();

        // Now that we have the hash, store the updated event in memory
        self.events.push(event.clone());
        self.current_hash = Some(event.hash.clone());

        // Notify runtime if callback is set
        if let Some(callback) = &self.event_callback {
            if let Err(err) = callback.send(event.clone()) {
                tracing::warn!("Failed to notify runtime of event: {}", err);
            }
        }
        // Update chain head
        let head_label = format!("{}:chain-head", self.actor_id);

        // Set new head
        self.content_store
            .replace_at_label(head_label, content_ref.clone())
            .await;

        tracing::debug!(
            "Stored event {} in content store for actor {}",
            content_ref.hash(),
            self.actor_id
        );

        Ok(event)
    }

    /// Internal method to finalize event (add hash) and store it
    fn finalize_and_store_event(
        &mut self,
        mut event: ChainEvent,
    ) -> Result<ChainEvent, serde_json::Error> {
        let mut hasher = Sha1::new();

        // Hash previous state + event data
        if let Some(prev_hash) = &self.current_hash {
            hasher.update(prev_hash);
        }
        hasher.update(&event.data);

        // Set the hash
        event.hash = hasher.finalize().to_vec();

        // Store the event
        self.events.push(event.clone());
        self.current_hash = Some(event.hash.clone());

        // Notify runtime if callback is set
        if let Some(callback) = &self.event_callback {
            if let Err(err) = callback.send(event.clone()) {
                tracing::warn!("Failed to notify runtime of event: {}", err);
            }
        }

        Ok(event)
    }

    // Convenience methods for specific event types

    pub async fn add_http_event(
        &mut self,
        event: HttpEvent,
    ) -> Result<ChainEvent, serde_json::Error> {
        self.add_typed_event(event).await
    }

    pub async fn add_message_event(
        &mut self,
        event: MessageEvent,
    ) -> Result<ChainEvent, serde_json::Error> {
        self.add_typed_event(event).await
    }

    pub async fn add_filesystem_event(
        &mut self,
        event: FilesystemEvent,
    ) -> Result<ChainEvent, serde_json::Error> {
        self.add_typed_event(event).await
    }

    pub async fn add_runtime_event(
        &mut self,
        event: RuntimeEvent,
    ) -> Result<ChainEvent, serde_json::Error> {
        self.add_typed_event(event).await
    }

    pub async fn add_supervisor_event(
        &mut self,
        event: SupervisorEvent,
    ) -> Result<ChainEvent, serde_json::Error> {
        self.add_typed_event(event).await
    }

    // Legacy method for backward compatibility
    pub fn add_event(&mut self, event_type: String, data: Vec<u8>) -> ChainEvent {
        let event = ChainEvent {
            hash: Vec::new(), // Will be set below
            parent_hash: self.current_hash.clone(),
            event_type,
            data,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            description: None,
        };

        // No error handling for backward compatibility
        match self.finalize_and_store_event(event) {
            Ok(e) => e,
            Err(err) => {
                tracing::error!("Error storing event: {}", err);
                ChainEvent {
                    hash: Vec::new(),
                    parent_hash: self.current_hash.clone(),
                    event_type: "error".to_string(),
                    data: Vec::new(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    description: Some(format!("Error storing event: {}", err)),
                }
            }
        }
    }

    pub fn verify(&self) -> bool {
        let mut prev_hash = None;

        for event in &self.events {
            let mut hasher = Sha1::new();

            if let Some(ph) = &prev_hash {
                hasher.update(ph);
            }
            hasher.update(&event.data);

            let computed_hash = hasher.finalize().to_vec();
            if computed_hash != event.hash {
                return false;
            }

            prev_hash = Some(event.hash.clone());
        }

        true
    }

    pub fn set_event_callback(&mut self, callback: mpsc::Sender<ChainEvent>) {
        self.event_callback = Some(callback);
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let chain: StateChain = serde_json::from_str(&json)?;
        Ok(chain)
    }

    pub fn get_last_event(&self) -> Option<&ChainEvent> {
        self.events.last()
    }

    pub fn get_events(&self) -> &[ChainEvent] {
        &self.events
    }
}
