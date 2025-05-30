# Random ID System for Theater Actors

## Description
- Implementing a cryptographically secure random ID generation system for actors
- This change is necessary to prevent ID collisions in large-scale deployments
- Expected benefits include improved security and reliability in distributed environments
- Alternative considered: Sequential IDs (rejected due to predictability concerns)

## Working Notes
- Initially explored using UUID v4, but found it to be unnecessarily large for our needs
- Decided on a 16-byte random ID with base64url encoding for string representation
- Challenge encountered: Random generation in WASM has limited entropy sources
- Solution: Using the host system's CSPRNG via a new host function
- Implementation involved:
  - New host function: `generate_secure_random(length: u32) -> Vec<u8>`
  - Actor API wrapper: `fn generate_actor_id() -> String`
  - ID validation utilities: `fn validate_actor_id(id: &str) -> bool`

- Commit references:
  - Added host function: `a1b2c3d4`
  - WASM bindings: `e5f6g7h8`
  - API wrappers: `i9j0k1l2`

## Final Notes
- Final implementation uses 16-byte random IDs with base64url encoding
- String representation is 22 characters (no padding)
- Added validation to ensure IDs follow expected format
- All actors now use this ID system by default
- Backward compatibility maintained for existing actors with legacy IDs
- Learned that cryptographic RNG in WASM requires careful consideration
- Future considerations:
  - Consider adding ID collision detection in large-scale deployments
  - Explore hierarchical ID systems for parent-child relationships
  - Implement ID reservation system for planned actor deployments
