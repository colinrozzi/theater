# Random ID System and Message Passing

## Description
We are implementing a new random ID system and updating the message passing infrastructure to support better supervision capabilities.

### Changes Include
- Switching from string-based IDs to UUID-based TheaterId
- Updating message passing system to route through supervisors
- Preparing infrastructure for chain events and lifecycle events

### Motivation
The current string-based ID system doesn't guarantee uniqueness and makes supervision hierarchies harder to implement. Random UUIDs will provide guaranteed unique identifiers and help establish cleaner parent-child relationships between actors.

### Expected Benefits
- Guaranteed unique actor identification
- Better supervision hierarchy support
- Cleaner message routing
- Foundation for robust actor lifecycle management

### Potential Risks
- Migration complexity for existing actors
- Performance overhead of UUID generation
- Backward compatibility concerns

## Working Notes

### 2025-01-29
- Created new TheaterId type using UUID v4
- Updated messages.rs to use TheaterId instead of String
- Modified actor_runtime.rs and theater_runtime.rs to support new ID system
- Added serialization support via serde
- Encountered and fixed UUID serialization issue by adding serde feature
- Next steps: Complete testing and integration

### Implementation Details
1. Created new `id.rs` module
2. Updated key components:
   - messages.rs
   - actor_runtime.rs
   - theater_runtime.rs
   - store.rs
3. Added UUID dependency with serde support
4. Modified actor spawning to use new ID system

## Final Notes
[To be completed when the change is finished]
