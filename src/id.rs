use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// A unique identifier for entities within the theater system
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TheaterId(Uuid);

impl TheaterId {
    /// Generate a new random ID
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a TheaterId from a string
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    /// Get the underlying UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl FromStr for TheaterId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl fmt::Display for TheaterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_unique() {
        let id1 = TheaterId::generate();
        let id2 = TheaterId::generate();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_parse_and_display() {
        let id = TheaterId::generate();
        let id_str = id.to_string();
        let parsed = TheaterId::from_str(&id_str).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_serialization() {
        let id = TheaterId::generate();
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized: TheaterId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);
    }
}
