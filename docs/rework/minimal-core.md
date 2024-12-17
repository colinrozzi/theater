# Minimal Core Implementation Guide

## Overview

This guide outlines the essential components needed for a minimal but complete implementation of Theater. It focuses on the core functionality that enables verifiable, deterministic actors while leaving room for future extensions.

## Core Components

```
┌─────────────────────────┐
│     Runtime Manager     │
├─────────────────────────┤
│    Actor Instance   │   │
│    ┌───────────┐   │   │
│    │   WASM    │   │   │
│    │ Component │   │   │
│    └───────────┘   │   │
│          │         │   │
│    ┌───────────┐   │   │
│    │  Chain    │   │   │
│    │ Manager   │   │   │
│    └───────────┘   │   │
└─────────────────────────┘
```

## 1. Runtime Manager

The runtime manager is the entry point and coordinator.

```rust
pub struct RuntimeManager {
    // Active actor instances
    actors: HashMap<ActorId, ActorInstance>,
    
    // Component loading and validation
    component_loader: ComponentLoader,
    
    // Basic message routing
    router: MessageRouter,
}

impl RuntimeManager {
    /// Create new runtime instance
    pub fn new() -> Self {
        RuntimeManager {
            actors: HashMap::new(),
            component_loader: ComponentLoader::new(),
            router: MessageRouter::new(),
        }
    }

    /// Start a new actor from a WASM component
    pub async fn start_actor(
        &mut self,
        id: ActorId,
        component_bytes: &[u8],
        config: Option<String>,
    ) -> Result<()> {
        // Load and validate component
        let component = self.component_loader.load(component_bytes)?;
        
        // Create chain for the actor
        let chain = ChainManager::new(id.clone())?;
        
        // Initialize actor instance
        let instance = ActorInstance::new(component, chain)?;
        instance.initialize(config)?;
        
        // Store actor
        self.actors.insert(id, instance);
        Ok(())
    }

    /// Handle message routing between actors
    pub async fn route_message(&mut self, message: Message) -> Result<()> {
        self.router.route_message(message, &mut self.actors)?;
        Ok(())
    }
}
```

## 2. Actor Instance

Actor instances manage individual WASM components and their associated chains.

```rust
pub struct ActorInstance {
    // The WASM component instance
    component: WasmComponent,
    
    // The actor's chain
    chain: ChainManager,
    
    // Current verified state
    state: ActorState,
}

impl ActorInstance {
    /// Create new actor instance
    pub fn new(component: WasmComponent, chain: ChainManager) -> Result<Self> {
        Ok(ActorInstance {
            component,
            chain,
            state: ActorState::default(),
        })
    }
    
    /// Initialize the actor
    pub fn initialize(&mut self, config: Option<String>) -> Result<()> {
        // Call init on the component
        let initial_state = self.component.init(config)?;
        
        // Create genesis event
        self.chain.record_genesis(initial_state.clone())?;
        
        // Set initial state
        self.state = initial_state;
        Ok(())
    }
    
    /// Handle an input
    pub async fn handle(&mut self, input: Input) -> Result<Output> {
        // Get current state hash
        let state_before = self.chain.compute_state_hash(&self.state);
        
        // Handle input in component
        let (output, new_state) = self.component.handle(input.clone(), self.state.clone())?;
        
        // Verify new state
        if !self.component.verify_state(&new_state) {
            return Err(Error::InvalidState);
        }
        
        // Record state transition
        let state_after = self.chain.compute_state_hash(&new_state);
        self.chain.record_event(Event {
            event_type: EventType::StateTransition,
            input: Some(input),
            state_before,
            state_after,
            timestamp: timestamp(),
        })?;
        
        // Update state
        self.state = new_state;
        
        Ok(output)
    }
}
```

## 3. Chain Manager

The chain manager handles chain storage and verification.

```rust
pub struct ChainManager {
    // Chain storage
    storage: ChainStorage,
    
    // Current chain head
    head: ChainElement,
}

impl ChainManager {
    /// Create new chain
    pub fn new(id: ActorId) -> Result<Self> {
        let storage = ChainStorage::new(id)?;
        let head = ChainElement::genesis();
        
        Ok(ChainManager {
            storage,
            head,
        })
    }
    
    /// Record new event
    pub fn record_event(&mut self, event: Event) -> Result<()> {
        // Create new chain element
        let element = ChainElement {
            event,
            previous_hash: Some(self.head.hash),
            sequence: self.head.sequence + 1,
            hash: Hash::default(), // Computed below
        };
        
        // Compute and set hash
        let hash = element.compute_hash()?;
        element.hash = hash;
        
        // Store element
        self.storage.append(element.clone())?;
        self.head = element;
        
        Ok(())
    }
    
    /// Verify chain
    pub fn verify(&self) -> Result<bool> {
        let mut previous: Option<&ChainElement> = None;
        
        for element in self.storage.iter() {
            // Verify sequence
            if let Some(prev) = previous {
                if element.sequence != prev.sequence + 1 {
                    return Ok(false);
                }
            }
            
            // Verify hash links
            if element.previous_hash != previous.map(|p| p.hash) {
                return Ok(false);
            }
            
            // Verify element hash
            if element.compute_hash()? != element.hash {
                return Ok(false);
            }
            
            previous = Some(element);
        }
        
        Ok(true)
    }
}
```

## 4. Component Loader

The component loader manages WASM component lifecycle.

```rust
pub struct ComponentLoader {
    // WASM engine
    engine: wasmtime::Engine,
}

impl ComponentLoader {
    /// Load and validate a component
    pub fn load(&self, bytes: &[u8]) -> Result<WasmComponent> {
        // Parse component
        let component = Component::new(&self.engine, bytes)?;
        
        // Validate interfaces
        self.validate_interfaces(&component)?;
        
        // Create instance
        let instance = WasmComponent::instantiate(component)?;
        
        Ok(instance)
    }
    
    /// Validate required interfaces
    fn validate_interfaces(&self, component: &Component) -> Result<()> {
        // Check for required exports
        if !component.exports().contains("init") {
            return Err(Error::MissingInterface("init"));
        }
        if !component.exports().contains("handle") {
            return Err(Error::MissingInterface("handle"));
        }
        if !component.exports().contains("verify_state") {
            return Err(Error::MissingInterface("verify_state"));
        }
        
        Ok(())
    }
}
```

## Minimal Implementation Checklist

1. **Runtime Core**
   - [ ] Basic actor management
   - [ ] Component loading
   - [ ] Message routing
   - [ ] Error handling

2. **Actor Implementation**
   - [ ] State management
   - [ ] Input handling
   - [ ] Component interface
   - [ ] Verification

3. **Chain Management**
   - [ ] Chain storage
   - [ ] Event recording
   - [ ] Hash verification
   - [ ] State tracking

4. **Component System**
   - [ ] WASM loading
   - [ ] Interface validation
   - [ ] Instance management
   - [ ] Resource control

## Next Steps

After implementing the minimal core:

1. **Testing**
   - Write unit tests
   - Add integration tests
   - Test chain verification
   - Stress test components

2. **Extensions**
   - Add HTTP interface
   - Implement WebSocket support
   - Add persistence options
   - Create monitoring tools

3. **Documentation**
   - API documentation
   - User guides
   - Example actors
   - Best practices

4. **Tools**
   - Chain explorer
   - State debugger
   - Performance monitors
   - Development utilities