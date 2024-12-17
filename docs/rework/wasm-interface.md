# WebAssembly Component Interface

## Overview

The WebAssembly component interface defines how actors are implemented and how they interact with the Theater runtime. This specification uses the WebAssembly Component Model and WIT (WebAssembly Interface Types) to define clear, language-agnostic interfaces.

## Core Interfaces

### Actor Interface

```wit
// theater-actor.wit
package theater:actor

/// Base actor interface that all actors must implement
interface actor {
    /// Actor state type (must be serializable)
    type state

    /// Initialize the actor with optional config
    init: func(config: option<string>) -> result<state, error>
    
    /// Handle an input, producing new state
    handle: func(
        input: input,
        current-state: state
    ) -> result<(output, state), error>
    
    /// Verify if a given state is valid
    verify-state: func(state: state) -> bool
}

/// Possible inputs to an actor
variant input {
    /// Direct message from another actor
    message(message),
    
    /// Custom input type
    custom(string)
}

/// Actor outputs
variant output {
    /// Message to send to another actor
    message(message),
    
    /// No output produced
    none,
    
    /// Custom output type
    custom(string)
}

/// Message structure for actor communication
record message {
    /// Target actor (for sends)
    target: option<string>,
    
    /// Content of the message
    content: string,
    
    /// Hash of sender's chain head
    chain-head: option<hash>
}

/// Error types
variant error {
    /// Invalid input received
    invalid-input(string),
    
    /// State transition failed
    state-error(string),
    
    /// General error
    other(string)
}

/// Hash type for chain references
type hash = string
```

### Chain Interface

```wit
// theater-chain.wit
package theater:chain

/// Interface for interacting with the actor's chain
interface chain {
    /// Get the current chain head
    get-head: func() -> hash
    
    /// Get a specific element by sequence number
    get-element: func(sequence: u64) -> option<chain-element>
    
    /// Verify the chain up to a point
    verify: func(up-to: option<u64>) -> result<bool, error>
    
    /// Get the current verified state
    get-verified-state: func() -> result<state, error>
}

/// Chain element structure
record chain-element {
    /// Event data
    event: event,
    
    /// Previous element's hash
    previous: option<hash>,
    
    /// This element's hash
    hash: hash,
    
    /// Sequence number
    sequence: u64
}

/// Event structure
record event {
    /// Type of event
    type: event-type,
    
    /// Input that caused the event (if any)
    input: option<string>,
    
    /// State before the event
    state-before: hash,
    
    /// State after the event
    state-after: hash,
    
    /// Event timestamp
    timestamp: u64
}

/// Event types
variant event-type {
    /// Initial state creation
    genesis,
    
    /// State transition
    state-transition,
    
    /// Message received
    message-received(message-info),
    
    /// Message sent
    message-sent(message-info),
    
    /// Custom event
    custom(string)
}

/// Message event information
record message-info {
    /// Other actor involved
    actor: string,
    
    /// Message hash
    message-hash: hash
}
```

## Implementation Requirements

### Actor Implementation

1. **State Management**
   - State must be serializable
   - State changes must be deterministic
   - State verification must be consistent

2. **Input Handling**
   - Must handle all input types
   - Must be deterministic
   - Must produce valid state transitions

3. **Error Handling**
   - Must return appropriate errors
   - Must maintain state consistency
   - Must not panic

### Example Actor (Pseudo-Rust)

```rust
#[component]
impl Actor for CounterActor {
    type State = u64;

    fn init(_config: Option<String>) -> Result<Self::State, Error> {
        Ok(0)
    }

    fn handle(
        &self,
        input: Input,
        state: Self::State
    ) -> Result<(Output, Self::State), Error> {
        match input {
            Input::Message(msg) => {
                match msg.content.as_str() {
                    "increment" => Ok((Output::None, state + 1)),
                    "decrement" => Ok((Output::None, state - 1)),
                    _ => Err(Error::InvalidInput("unknown command".into()))
                }
            },
            Input::Custom(_) => Err(Error::InvalidInput("custom not supported".into()))
        }
    }

    fn verify_state(&self, state: Self::State) -> bool {
        // For this simple actor, all u64 values are valid
        true
    }
}
```

## Runtime Integration

### Component Loading

1. Runtime must:
   - Load WASM component
   - Validate interface implementation
   - Initialize actor state
   - Set up chain storage

2. Component must:
   - Export required interfaces
   - Handle initialization
   - Maintain determinism

### State Management

1. Runtime responsibilities:
   - Store state securely
   - Verify state transitions
   - Maintain chain integrity
   - Handle persistence

2. Component responsibilities:
   - Implement state transitions
   - Verify state validity
   - Handle serialization
   - Maintain determinism

## Communication Patterns

### Direct Messaging

```rust
// Send a message
let output = actor.handle(Input::Message(Message {
    target: Some("other-actor"),
    content: "hello",
    chain_head: Some(current_head),
}))?;

// Receive a message
let (output, new_state) = actor.handle(Input::Message(Message {
    target: None,
    content: received_content,
    chain_head: Some(sender_head),
}))?;
```

### Custom Interactions

1. Define custom input/output types
2. Implement handlers in actor
3. Register with runtime
4. Maintain verifiability

## Security Considerations

1. **Isolation**
   - Components must be fully isolated
   - No direct memory access
   - Controlled resource usage
   - Secure state storage

2. **Determinism**
   - No external sources of randomness
   - Consistent execution across platforms
   - Verifiable state transitions

3. **Resource Control**
   - Memory limits
   - Computation limits
   - Storage quotas
   - Message rate limiting

## Best Practices

1. **State Design**
   - Keep state minimal
   - Make state easily verifiable
   - Use efficient serialization
   - Plan for upgrades

2. **Error Handling**
   - Provide clear error messages
   - Maintain state consistency
   - Handle all edge cases
   - Fail gracefully

3. **Testing**
   - Test state transitions
   - Verify determinism
   - Check error cases
   - Test chain verification