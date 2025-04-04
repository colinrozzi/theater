//! # Theater ID System
//!
//! This module provides the `TheaterId` type, which is used throughout the Theater system
//! to uniquely identify actors, resources, and other entities. The IDs are based on UUIDs
//! to ensure uniqueness across distributed environments.
//!
//! ## Example
//!
//! ```rust
//! use theater::id::TheaterId;
//! use std::str::FromStr;
//!
//! // Generate a new random ID
//! let id = TheaterId::generate();
//!
//! // Convert to string and back
//! let id_str = id.to_string();
//! let parsed_id = TheaterId::from_str(&id_str).unwrap();
//! assert_eq!(id, parsed_id);
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// # TheaterId
///
/// A unique identifier for entities within the Theater system, including actors,
/// channels, and resources.
///
/// ## Purpose
///
/// TheaterId provides a type-safe way to identify and reference entities within the Theater
/// system. It helps prevent confusion between different types of IDs and enables strong
/// type checking at compile time.
///
/// ## Implementation Notes
///
/// Internally, TheaterId is implemented using UUIDs (Universally Unique Identifiers)
/// to ensure uniqueness across distributed systems without requiring central coordination.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TheaterId(Uuid);

impl TheaterId {
    /// Generates a new random TheaterId.
    ///
    /// ## Purpose
    ///
    /// This method creates a new, globally unique identifier that can be used to
    /// identify an actor, channel, or other entity in the Theater system.
    ///
    /// ## Returns
    ///
    /// A new TheaterId instance with a random UUID.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::id::TheaterId;
    ///
    /// let id = TheaterId::generate();
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This method uses the UUID v4 algorithm, which generates IDs using random numbers.
    /// This ensures that IDs are unique across different machines without coordination.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses a TheaterId from a string.
    ///
    /// ## Parameters
    ///
    /// * `s` - The string to parse
    ///
    /// ## Returns
    ///
    /// * `Ok(TheaterId)` - The successfully parsed TheaterId
    /// * `Err(uuid::Error)` - An error occurred during parsing
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::id::TheaterId;
    ///
    /// let id_result = TheaterId::parse("550e8400-e29b-41d4-a716-446655440000");
    /// assert!(id_result.is_ok());
    ///
    /// let invalid_result = TheaterId::parse("not-a-valid-uuid");
    /// assert!(invalid_result.is_err());
    /// ```
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    /// Gets the underlying UUID.
    ///
    /// ## Returns
    ///
    /// A reference to the underlying Uuid.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::id::TheaterId;
    ///
    /// let id = TheaterId::generate();
    /// let uuid = id.as_uuid();
    /// ```
    ///
    /// ## Purpose
    ///
    /// This method allows access to the underlying UUID, which can be useful for
    /// interoperating with other systems or libraries that work with UUIDs directly.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

/// Implementation of the FromStr trait for TheaterId.
///
/// This allows parsing a TheaterId from a string using the standard FromStr trait,
/// which enables using the `parse` method on strings and the `?` operator for error handling.
///
/// ## Example
///
/// ```rust
/// use theater::id::TheaterId;
/// use std::str::FromStr;
///
/// let id = TheaterId::from_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
/// ```
impl FromStr for TheaterId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Implementation of the Display trait for TheaterId.
///
/// This allows converting a TheaterId to a string using the standard Display trait,
/// which enables using it with string formatting macros like `format!`, `println!`, etc.
///
/// ## Example
///
/// ```rust
/// use theater::id::TheaterId;
///
/// let id = TheaterId::generate();
/// let id_string = format!("{}", id);  // Converts to hyphenated UUID string
/// println!("Actor ID: {}", id);      // Prints the ID in a readable format
/// ```
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
