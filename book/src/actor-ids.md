# Actor ID System

The Theater Actor ID system provides a secure, unique identifier mechanism for all entities within the Theater ecosystem. This document covers how IDs are generated, managed, and used throughout the system.

## Overview

Theater uses UUIDs (Universally Unique Identifiers) to create unique, cryptographically secure identifiers for actors and other entities. These IDs ensure:

- Uniqueness across distributed systems
- Collision resistance even in large-scale deployments
- Unpredictability for security purposes
- Consistent formatting and representation

## The TheaterId Type

The core of the ID system is the `TheaterId` type, which encapsulates a UUID and provides convenient methods for working with actor identifiers:

```rust
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
```

## ID Generation

Actor IDs are generated using the UUID v4 format, which provides:

- 128 bits (16 bytes) of random data
- Extremely low collision probability (1 in 2^122)
- Standardized string representation

Example of generating a new actor ID:

```rust
let actor_id = TheaterId::generate();
```

## ID String Representation

IDs are represented as standard UUID strings:

```rust
// Convert ID to string
let id_string = actor_id.to_string();  // Format: "550e8400-e29b-41d4-a716-446655440000"

// Parse string back to ID
let parsed_id = TheaterId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
```

## Serialization Support

Actor IDs are designed to work seamlessly with serde for JSON serialization:

```rust
#[derive(Serialize, Deserialize)]
struct ActorState {
    id: TheaterId,
    // Other state fields...
}
```

## Using Actor IDs

### In Actor Manifests

Actor IDs can be referenced in manifest files:

```toml
[dependencies]
parent_actor = "550e8400-e29b-41d4-a716-446655440000"
```

### In Message Routing

IDs are used for message routing between actors:

```rust
// Send a message to a specific actor by ID
theater_runtime::send_message_to_actor(&target_id, message);
```

### In Supervision

Parent actors reference children by their IDs:

```rust
// Get a child actor's status
let status = supervisor::get_child_status(&child_id)?;
```

## ID Validation

When working with IDs from external sources, always validate them:

```rust
match TheaterId::parse(input_string) {
    Ok(id) => {
        // Valid ID, proceed with operation
    },
    Err(_) => {
        // Invalid ID, handle the error
    }
}
```

## Best Practices

1. **Never Generate IDs Manually**
   - Always use `TheaterId::generate()` to ensure proper randomness

2. **Store Full IDs**
   - Don't truncate or modify IDs as this reduces their uniqueness properties

3. **Use Type Safety**
   - Prefer the `TheaterId` type over raw strings when possible
   - This provides compile-time guarantees and better error handling

4. **Handle Parse Errors**
   - Always check for errors when parsing IDs from strings
   - Invalid IDs should be treated as authentication/authorization failures

5. **Include IDs in Logs**
   - Log actor IDs with operations for easier debugging
   - Use the string representation in log entries

## Implementation Notes

- The ID system uses the `uuid` crate with the `v4` feature for generation
- The implementation includes comprehensive tests for generation, parsing, and serialization
- Future enhancements may include:
  - Alternative ID formats for specific use cases
  - ID collision detection for large-scale deployments
  - Hierarchical ID systems for parent-child relationships

## Planned Enhancements

> **Note**: The following enhancements are planned for future releases:

1. **Secure Random ID System**: A new system using 16-byte random IDs with base64url encoding 
2. **Host CSPRNG Integration**: Using the host system's cryptographically secure random number generator
3. **Improved Format**: Shorter string representation (22 characters vs 36 for UUIDs)
4. **Backward Compatibility**: Support for both new and legacy ID formats