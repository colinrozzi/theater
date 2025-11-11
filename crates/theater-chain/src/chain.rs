use crate::event::Event;

pub struct Chain {
    events: Vec<Event>,
    current_hash: Option<Vec<u8>>,
}

impl Chain {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            current_hash: None,
        }
    }

    /// Adds a new typed event to the chain.
    ///
    /// ## Purpose
    ///
    /// This method adds a new event to the chain, handling the cryptographic linking
    /// and content-addressed storage aspects. It ensures the event is properly linked
    /// to its parent, calculates its hash, and notifies the Theater runtime about
    /// the new event.
    ///
    /// ## Parameters
    ///
    /// * `event_data` - The typed event data to add to the chain
    ///
    /// ## Returns
    ///
    /// * `Ok(ChainEvent)` - The newly created and added event with its hash
    /// * `Err(serde_json::Error)` - If there was an error serializing the event
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::chain::StateChain;
    /// # use theater::events::{ChainEventData, EventData};
    /// # use theater::id::TheaterId;
    /// # use tokio::sync::mpsc;
    /// # use anyhow::Result;
    /// #
    /// # async fn example() -> Result<()> {
    /// # let (theater_tx, _) = mpsc::channel(100);
    /// # let actor_id = TheaterId::generate();
    /// # let mut chain = StateChain::new(actor_id, theater_tx);
    /// #
    /// // Create event data
    /// let event_data = ChainEventData {
    ///     event_type: "state_change".to_string(),
    ///     data: EventData::Runtime(theater::events::runtime::RuntimeEventData::Log { level: "info".to_string(), message: "state changed".to_string() }),
    ///     timestamp: chrono::Utc::now().timestamp() as u64,
    ///     description: Some("Actor state changed".to_string()),
    /// };
    ///
    /// // Add the event to the chain
    /// let event = chain.add_typed_event(event_data)?;
    /// println!("Added event with hash: {:?}", event.hash);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Security
    ///
    /// This method enforces the append-only nature of the event chain by automatically
    /// linking each new event to the previous one through hash references. The hash of
    /// each event is calculated from its content, creating a content-addressed system
    /// that allows for verification of the entire history.
    ///
    /// ## Implementation Notes
    ///
    /// The method spawns an asynchronous task to notify the Theater runtime about the
    /// new event. This is done asynchronously to avoid blocking the caller waiting for
    /// the notification to be delivered.
    pub fn add_typed_event(
        &mut self,
        event_data: ChainEventData,
    ) -> Result<ChainEvent, serde_json::Error> {
        // Create initial event structure without hash
        let mut event = event_data.to_chain_event(self.current_hash.clone());

        // Store the event data in the content store
        let serialized_event = serde_json::to_vec(&event)?;
        let content_ref = ContentRef::from_content(&serialized_event);

        // Get the hash from ContentRef and use it as the event hash
        let hash_bytes = hex::decode(content_ref.hash()).unwrap();
        event.hash = hash_bytes.clone();

        // Now that we have the hash, store the updated event in memory
        self.events.push(event.clone());
        self.current_hash = Some(event.hash.clone());

        // notify the runtime of the event
        let evt = event.clone();
        let id = self.actor_id.clone();
        let tx = self.theater_tx.clone();
        tokio::spawn(async move {
            debug!("actor [{}]: Sending event {} to runtime", id, evt);
            tx.send(TheaterCommand::NewEvent {
                actor_id: id.clone(),
                event: evt.clone(),
            })
            .await
            .expect("Failed to send event to runtime");
            debug!("Sent event {} to runtime", hex::encode(evt.hash.clone()));
        });

        // I am removing storing the events in the content store for now because they are
        // accumulating too quickly. I need to build out the store local to each actor to store its
        // event that is cleaned up when the actor dies.
        /*
        let head_label = format!("{}:chain-head", self.actor_id);
        let content_store = self.content_store.clone();
        let prev_content_ref = content_ref.clone();

        tokio::spawn(async move {
            let stored_content_ref = content_store.store(serialized_event).await.unwrap();
            if stored_content_ref.hash() != prev_content_ref.hash() {
                tracing::error!(
                    "Content store hash mismatch: expected {}, got {}",
                    prev_content_ref.hash(),
                    stored_content_ref.hash()
                );
            }
            // Update chain head
            let _ = content_store
                .replace_at_label(head_label, stored_content_ref)
                .await;
        });
        */

        debug!(
            "Stored event {} in content store for actor {}",
            content_ref.hash(),
            self.actor_id
        );

        Ok(event)
    }

    /// Verifies the integrity of the entire event chain.
    ///
    /// ## Purpose
    ///
    /// This method validates the cryptographic integrity of the entire event chain
    /// by recalculating each event's hash and ensuring it matches the stored hash,
    /// and by verifying that each event correctly references its parent. This allows
    /// for detecting any tampering with the event history.
    ///
    /// ## Returns
    ///
    /// * `true` - If the chain is valid and all links are intact
    /// * `false` - If any event's hash is invalid or parent links are broken
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::chain::StateChain;
    /// # use theater::id::TheaterId;
    /// # use tokio::sync::mpsc;
    /// #
    /// # fn example() {
    /// # let (theater_tx, _) = mpsc::channel(100);
    /// # let actor_id = TheaterId::generate();
    /// # let chain = StateChain::new(actor_id, theater_tx);
    /// #
    /// // Check if the chain is valid
    /// if chain.verify() {
    ///     println!("Chain integrity verified");
    /// } else {
    ///     println!("Chain verification failed - possible tampering detected");
    /// }
    /// # }
    /// ```
    ///
    /// ## Security
    ///
    /// This method is a critical security feature that enables detecting any
    /// tampering or corruption in the event history. It recalculates each event's
    /// hash based on its content and checks it against the stored hash, making it
    /// impossible to modify an event without detection.
    ///
    /// ## Implementation Notes
    ///
    /// The verification process ensures that:
    /// 1. Each event's hash matches its content
    /// 2. Each event's parent hash matches the previous event's hash
    /// 3. The chain of parent references is unbroken
    pub fn verify(&self) -> bool {
        let mut prev_hash = None;

        for event in &self.events {
            // Create a temporary event with everything except the hash
            let temp_event = ChainEvent {
                hash: vec![],
                parent_hash: prev_hash.clone(),
                event_type: event.event_type.clone(),
                data: event.data.clone(),
                timestamp: event.timestamp,
                description: event.description.clone(),
            };

            // Serialize the event (just like in add_typed_event)
            let serialized_event = match serde_json::to_vec(&temp_event) {
                Ok(data) => data,
                Err(_) => return false,
            };

            // Calculate hash using ContentRef (same as in add_typed_event)
            let content_ref = ContentRef::from_content(&serialized_event);
            let hash_bytes = match hex::decode(content_ref.hash()) {
                Ok(bytes) => bytes,
                Err(_) => return false,
            };

            // Verify this hash matches the stored hash
            if hash_bytes != event.hash {
                return false;
            }

            prev_hash = Some(event.hash.clone());
        }

        true
    }

    /// Saves the entire state chain to a JSON file.
    ///
    /// ## Purpose
    ///
    /// This method serializes the state chain to a JSON file, allowing the event
    /// history to be persisted for later analysis, debugging, or backup purposes.
    /// The saved file can be loaded for offline verification or examination.
    ///
    /// ## Parameters
    ///
    /// * `path` - Path where the JSON file will be written
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - If the file was successfully written
    /// * `Err(anyhow::Error)` - If there was an error serializing or writing the file
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::chain::StateChain;
    /// # use theater::id::TheaterId;
    /// # use tokio::sync::mpsc;
    /// # use std::path::Path;
    /// # use anyhow::Result;
    /// #
    /// # fn example() -> Result<()> {
    /// # let (theater_tx, _) = mpsc::channel(100);
    /// # let actor_id = TheaterId::generate();
    /// # let chain = StateChain::new(actor_id, theater_tx);
    /// #
    /// // Save the chain to a file
    /// chain.save_to_file(Path::new("actor_events.json"))?;
    /// println!("Chain saved to file");
    /// # Ok(())
    /// # }
    /// ```
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn save_chain(&self) -> Result<()> {
        // this will be different than the save_to_file method, in that we are going to save each
        // of the events at their hash in the THEATER_DIR/events/{event_id} path, and then the chain (the
        // head of the chain) will be saved at the actor id in the THEATER_DIR/chains/{actor_id} path

        let theater_dir = std::env::var("THEATER_HOME").expect(
            "THEATER_DIR environment variable must be set to the directory where events are stored",
        );
        let events_dir = format!("{}/events", theater_dir);
        let chains_dir = format!("{}/chains", theater_dir);
        std::fs::create_dir_all(&events_dir)?;
        std::fs::create_dir_all(&chains_dir)?;
        let chain_path = format!("{}/{}", chains_dir, self.actor_id);

        // Save each event to its own file
        for event in &self.events {
            let event_path = format!("{}/{}", events_dir, hex::encode(&event.hash));
            std::fs::write(
                event_path,
                serde_json::to_string(event).expect("Failed to serialize event"),
            )
            .expect("Failed to write event file");
        }

        // Save the chain to a file
        std::fs::write(
            chain_path,
            serde_json::to_string(&self.current_hash).expect("Failed to serialize current hash"),
        )
        .expect("Failed to write chain file");
        Ok(())
    }

    /// Gets the most recent event in the chain.
    ///
    /// ## Purpose
    ///
    /// This method provides access to the most recent event in the chain, which
    /// represents the current state of the actor's execution history. It's useful
    /// for quickly accessing the latest event without having to traverse the entire
    /// chain.
    ///
    /// ## Returns
    ///
    /// * `Some(&ChainEvent)` - Reference to the most recent event, if the chain is not empty
    /// * `None` - If the chain is empty and there are no events
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::chain::StateChain;
    /// # use theater::id::TheaterId;
    /// # use tokio::sync::mpsc;
    /// #
    /// # fn example() {
    /// # let (theater_tx, _) = mpsc::channel(100);
    /// # let actor_id = TheaterId::generate();
    /// # let chain = StateChain::new(actor_id, theater_tx);
    /// #
    /// // Get the most recent event
    /// if let Some(last_event) = chain.get_last_event() {
    ///     println!("Last event type: {}", last_event.event_type);
    /// } else {
    ///     println!("No events in the chain yet");
    /// }
    /// # }
    /// ```
    pub fn get_last_event(&self) -> Option<&ChainEvent> {
        self.events.last()
    }

    /// Gets all events in the chain as an ordered slice.
    ///
    /// ## Purpose
    ///
    /// This method provides access to the complete event history in chronological
    /// order, allowing for traversal, analysis, or filtering of events. This is
    /// useful for debugging, auditing, or reconstructing actor state.
    ///
    /// ## Returns
    ///
    /// * `&[ChainEvent]` - A slice of all events in the chain, from oldest to newest
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::chain::StateChain;
    /// # use theater::id::TheaterId;
    /// # use tokio::sync::mpsc;
    /// #
    /// # fn example() {
    /// # let (theater_tx, _) = mpsc::channel(100);
    /// # let actor_id = TheaterId::generate();
    /// # let chain = StateChain::new(actor_id, theater_tx);
    /// #
    /// // Get all events and count them by type
    /// let events = chain.get_events();
    /// println!("Total events: {}", events.len());
    ///
    /// // Count events by type
    /// let mut type_counts = std::collections::HashMap::new();
    /// for event in events {
    ///     *type_counts.entry(event.event_type.clone()).or_insert(0) += 1;
    /// }
    ///
    /// // Print counts
    /// for (event_type, count) in type_counts {
    ///     println!("{}: {}", event_type, count);
    /// }
    /// # }
    /// ```
    pub fn get_events(&self) -> &[ChainEvent] {
        &self.events
    }
}
