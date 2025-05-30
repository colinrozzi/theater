# Rustdoc Documentation Proposal for Theater

## Description

This proposal outlines a comprehensive approach to documenting the Theater codebase using Rustdoc comments. The focus is on documenting all public items (functions, structs, enums, traits, etc.) throughout the codebase to support both users of the library and internal maintainers.

As the Theater project grows, comprehensive documentation becomes increasingly important for:
1. Onboarding new contributors
2. Maintaining code understanding for existing developers
3. Supporting users building applications with Theater
4. Ensuring design decisions and architecture are clearly communicated

### Why This Change Is Necessary

- **Knowledge Sharing**: Current lack of comprehensive documentation makes it difficult for new contributors to understand the system
- **Maintenance Support**: Without documentation, maintaining and updating code becomes increasingly challenging
- **API Usability**: Users of the library need clear documentation to effectively use the public API
- **Technical Debt**: Addressing documentation now prevents future technical debt and knowledge loss

### Expected Benefits

- Improved onboarding experience for new contributors
- Better developer experience for library users
- Reduced time spent understanding code for maintenance tasks
- Clear communication of design decisions and architectural patterns
- Enhanced IDE support through better doc comments

### Potential Risks

- Time investment required to properly document code
- Maintenance burden to keep documentation in sync with code changes
- Potential for documentation to become outdated if not consistently maintained

## Technical Approach

### Documentation Template for Public Items

Every public item should have documentation that includes the following sections:

```rust
/// # Short Description
///
/// A one or two sentence description of what this item does or represents.
///
/// ## Purpose
///
/// A detailed explanation of why this item exists and what role it plays in the system.
///
/// ## Example
///
/// ```rust
/// use theater::module_name::ItemName;
/// // Example code showing how to use this item
/// ```
///
/// ## Parameters
///
/// * `param1` - Description of the first parameter
/// * `param2` - Description of the second parameter
/// ...
///
/// ## Returns
///
/// Description of what this function returns, including possible error conditions.
///
/// ## Safety
///
/// Any safety considerations, especially for unsafe functions.
///
/// ## Security 
///
/// Security implications, especially for items that interact with user input or
/// WebAssembly code.
///
/// ## Implementation Notes
///
/// Details about the implementation that would be helpful for maintainers.
pub fn example_function(param1: Type1, param2: Type2) -> ReturnType {
    // Function body
}
```

Not all sections are applicable to all items. For example:
- Structs don't need "Returns" sections
- Safe functions don't need "Safety" sections
- Simple utilities might not need "Security" sections

### Priority Modules

Based on examining the codebase, here are the priority modules for documentation:

1. **Core Actor System**
   - `actor_executor.rs`
   - `actor_handle.rs`
   - `actor_runtime.rs`
   - `actor_store.rs`
   - `theater_runtime.rs`
   - `theater_server.rs`

2. **WebAssembly Integration**
   - `wasm.rs`
   - WIT interfaces in `/wit` directory

3. **Chain and Events**
   - `chain/mod.rs`
   - `events/mod.rs` (if exists)

4. **Core Data Structures**
   - `id.rs`
   - `messages.rs`
   - `config.rs`

5. **Handler Implementations**
   - `host/*.rs` files

### Sample Documentation for Key Items

Here are samples demonstrating the documentation format for different types of items:

#### For Theater Runtime

```rust
/// # TheaterRuntime
///
/// The main runtime for the Theater system, responsible for managing actors and their lifecycles.
///
/// ## Purpose
///
/// TheaterRuntime is the central component that coordinates actors within the Theater system.
/// It handles actor creation, destruction, and communication, providing the foundation for
/// the actor supervision system.
///
/// ## Example
///
/// ```rust
/// use theater::theater_runtime::TheaterRuntime;
///
/// async fn example() -> Result<(), Box<dyn std::error::Error>> {
///     // Create a new runtime with default configuration
///     let runtime = TheaterRuntime::new(Default::default())?;
///     
///     // Start an actor from a manifest
///     let actor_id = runtime.start_actor("path/to/manifest.toml", None).await?;
///     
///     // Later, stop the actor
///     runtime.stop_actor(&actor_id).await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Safety
///
/// This struct provides a safe interface to the WebAssembly actors. All potentially unsafe
/// operations involving WebAssembly execution are handled internally with appropriate
/// checks and validations.
///
/// ## Security
///
/// TheaterRuntime enforces sandbox boundaries for actors, preventing unauthorized
/// access to system resources. However, it's important to properly configure resource
/// limits to prevent denial-of-service attacks.
///
/// ## Implementation Notes
///
/// The runtime uses internal channels for communication between its components. Each actor
/// runs in a separate task to provide isolation and prevent blocking the main runtime.
pub struct TheaterRuntime {
    // Fields
}
```

#### For Actor Handle

```rust
/// # ActorHandle
///
/// A handle to an actor in the Theater system, providing methods to interact with the actor.
///
/// ## Purpose
///
/// ActorHandle provides a high-level interface for communicating with actors, managing their
/// lifecycle, and accessing their state and events. It encapsulates the details of message
/// passing and synchronization between the caller and the actor.
///
/// ## Example
///
/// ```rust
/// use theater::actor_handle::ActorHandle;
///
/// async fn example(handle: ActorHandle) -> Result<(), Box<dyn std::error::Error>> {
///     // Get the actor's current state
///     let state = handle.get_state().await?;
///     
///     // Execute an operation
///     let result = handle.execute_operation(/* operation parameters */).await?;
///     
///     // Stop the actor
///     handle.stop().await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Returns
///
/// Methods on ActorHandle typically return `Result<T, ActorError>` where:
/// - `T` is the successful result type
/// - `ActorError` indicates what went wrong if the operation fails
///
/// ## Safety
///
/// ActorHandle provides a safe interface to interact with actors. It handles
/// synchronization and message passing internally.
///
/// ## Security
///
/// ActorHandle enforces the actor's boundaries and permissions. Operations requested
/// through the handle are validated against the actor's capabilities.
///
/// ## Implementation Notes
///
/// ActorHandle uses channels internally to communicate with the actor's executor.
/// It implements Drop to ensure resources are properly cleaned up when the handle
/// is no longer needed.
pub struct ActorHandle {
    // Fields
}
```

#### For WIT Interface

```rust
/// # actor.wit
///
/// Defines the core interface that all Theater actors must implement.
///
/// ## Purpose
///
/// This interface establishes the contract between the Theater runtime and actor
/// implementations. Any WebAssembly component functioning as a Theater actor must
/// implement this interface.
///
/// ## Example
///
/// ```wit
/// // In a .wit file
/// use ntwk:theater/actor;
///
/// // Implementing the interface in Rust
/// struct MyActor;
/// impl Guest for MyActor {
///     fn init(state: State, params: (String,)) -> Result<(State,), String> {
///         // Implementation
///     }
/// }
/// ```
///
/// ## Parameters
///
/// * `state` - The current state of the actor, or None if first initialization
/// * `params` - Initialization parameters passed to the actor
///
/// ## Returns
///
/// * `Ok((State,))` - The updated state to store
/// * `Err(String)` - An error message if initialization fails
///
/// ## Security
///
/// This interface defines the entry point for actor execution. The runtime ensures
/// that actors cannot access resources outside their sandbox through this interface.
///
/// ## Implementation Notes
///
/// The state is passed as an opaque binary value, typically serialized/deserialized
/// using serde within the actor. The actor is responsible for maintaining state
/// integrity.
```

### Implementation Plan

1. **Phase 1: Core Module Documentation (First Week)**
   - Document `lib.rs` with a comprehensive overview
   - Document `actor_executor.rs`, `actor_handle.rs`, and `actor_runtime.rs`
   - Document `wasm.rs` and key interfaces in `/wit`

2. **Phase 2: Support Modules Documentation (Second Week)**
   - Document `chain/mod.rs` and related modules
   - Document `id.rs`, `messages.rs`, and `config.rs`
   - Document `theater_runtime.rs` and `theater_server.rs`

3. **Phase 3: Handler Documentation (Third Week)**
   - Document all handler implementations in `host/*.rs`
   - Document remaining modules

4. **Phase 4: Review and Finalization (Fourth Week)**
   - Review all documentation for consistency
   - Add cross-references between related items
   - Ensure examples compile and work correctly

### Documentation Standards and Guidelines

1. **Be Specific**: Avoid vague descriptions like "handles the actor" or "manages something". Explain exactly what the item does and how.

2. **Provide Context**: Explain how each item fits into the larger system architecture.

3. **Include Examples**: Every public struct, trait, and key function should have at least one usage example.

4. **Document Edge Cases**: Be explicit about error conditions, special cases, and limitations.

5. **Use Standard Markdown**: Use standard Markdown formatting in Rustdoc:
   - `# Heading 1` for the main title
   - `## Heading 2` for sections
   - Code blocks with triple backticks
   - Bullet lists with asterisks

6. **Cross-Reference**: Use links to other items where appropriate, e.g., `[TheaterRuntime]`.

7. **Document Constants and Enums**: Include documentation for all enum variants and public constants.

8. **Add Implementation Notes**: Include notes about implementation details that would be helpful for maintainers.

### Measuring Success

1. **Documentation Coverage**: We'll track the percentage of public items that have documentation.

2. **Quality Checks**: Ensure all documentation includes the required sections.

3. **Compilation Check**: Make sure code examples in documentation compile successfully.

4. **rustdoc Warnings**: Run `cargo doc --no-deps --document-private-items` and address all warnings.

### Tooling Integration

1. **CI Integration**: Add documentation checks to the CI pipeline.

2. **Documentation Tests**: Enable documentation tests to ensure examples remain valid.

3. **Style Checking**: Consider using a tool like `clippy` with documentation lints enabled.

## Next Steps

1. Set up a documentation template file in the project root as a reference.
2. Begin implementing documentation for the highest-priority modules.
3. Update the CI configuration to include documentation checks.
4. Create a tracking issue for documentation progress.
