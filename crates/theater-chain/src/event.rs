// use sha1::Digest;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::Hash;
use wasmtime::component::{ComponentType, Lift, Lower};

pub trait ChainEventData:
    Display + Debug + Send + Sync + ComponentType + Lift + Lower + Hash + Eq
{
    fn event_type(&self) -> String;
    fn len(&self) -> usize;
}

#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower, Hash, Eq)]
#[component(record)]
pub struct Event<D: ChainEventData> {
    /// Cryptographic hash of this event's content, used as its identifier.
    /// This is calculated based on all other fields except the hash itself.
    pub hash: Vec<u8>,
    /// Hash of the parent event, or None if this is the first event in the chain.
    /// This creates the cryptographic linking between events.
    #[component(name = "parent-hash")]
    pub parent_hash: Option<Vec<u8>>,
    /// The actual payload of the event, typically serialized structured data.
    pub data: D,
}

impl<D: ChainEventData> Display for Event<D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        println!("EVENT {}", hex::encode(&self.hash));
        match &self.parent_hash {
            Some(parent) => println!("{}", hex::encode(parent)),
            None => println!("0000000000000000"),
        }
        println!("{}", self.data.event_type());
        println!("{}", self.data.len());
        println!("");
        println!("{}", self.data);
        println!("\n\n");

        Ok(())
    }
}

// implement Eq for ChainEvent
impl<D: ChainEventData> PartialEq for Event<D> {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}
