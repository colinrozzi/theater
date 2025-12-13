use crate::event::Event;
use crate::event::EventType;
use tracing::error;

pub struct Chain<D: EventType> {
    events: Vec<Event<D>>,
    current_hash: Option<Vec<u8>>,
}

impl<D: EventType> Chain<D> {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            current_hash: None,
        }
    }

    pub fn add_typed_event(&mut self, event_data: D) -> Event<D> {
        let event = Event::new(self.current_hash.clone(), event_data);

        // Now that we have the hash, store the updated event in memory
        self.events.push(event.clone());
        self.current_hash = Some(event.hash.clone());

        event
    }

    pub fn verify(&self) -> bool {
        let mut prev_hash = None;

        for event in &self.events {
            // Verify the event's hash
            if !event.verify() {
                error!(
                    "Event hash verification failed for event {}",
                    hex::encode(&event.hash)
                );
                return false;
            }

            // Verify the parent hash linkage
            if event.parent_hash != prev_hash {
                error!(
                    "Parent hash mismatch for event {}: expected {:?}, found {:?}",
                    hex::encode(&event.hash),
                    prev_hash.as_ref().map(|h| hex::encode(h)),
                    event.parent_hash.as_ref().map(|h| hex::encode(h))
                );
                return false;
            }

            prev_hash = Some(event.hash.clone());
        }

        true
    }

    pub fn get_last_event(&self) -> Option<&Event<D>> {
        self.events.last()
    }

    pub fn get_events(&self) -> &[Event<D>] {
        &self.events
    }
}
