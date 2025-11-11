use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::Hash;
use wasmtime::component::{ComponentType, Lift, Lower};

pub trait EventData:
    Display + Debug + Send + Sync + ComponentType + Lift + Lower + Hash + Eq + Clone
{
    fn event_type(&self) -> String;
    fn len(&self) -> usize;
}

#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower, Hash, Eq)]
#[component(record)]
pub struct Event<D: EventData> {
    pub hash: Vec<u8>,
    #[component(name = "parent-hash")]
    pub parent_hash: Option<Vec<u8>>,
    pub data: D,
}

impl<D: EventData> Event<D> {
    pub fn new(parent_hash: Option<Vec<u8>>, data: D) -> Self {
        // Serialize the event data to compute its hash
        let mut hasher = Sha1::new();
        if let Some(ref parent) = parent_hash {
            hasher.update(parent);
        }
        hasher.update(data.to_string().as_bytes());
        let hash = hasher.finalize().to_vec();

        Self {
            hash,
            parent_hash,
            data,
        }
    }

    pub fn verify(&self) -> bool {
        let mut hasher = Sha1::new();
        if let Some(ref parent) = self.parent_hash {
            hasher.update(parent);
        }
        hasher.update(self.data.to_string().as_bytes());
        let computed_hash = hasher.finalize().to_vec();
        self.hash == computed_hash
    }
}

impl<D: EventData> Display for Event<D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        writeln!(f, "EVENT {}", hex::encode(&self.hash))?;
        match &self.parent_hash {
            Some(parent) => writeln!(f, "{}", hex::encode(parent))?,
            None => writeln!(f, "0000000000000000")?,
        }
        writeln!(f, "{}", self.data.event_type())?;
        writeln!(f, "{}", self.data.len())?;
        writeln!(f)?;
        writeln!(f, "{}", self.data)?;
        writeln!(f)?;
        Ok(())
    }
}

// implement Eq for ChainEvent
impl<D: EventData> PartialEq for Event<D> {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}
