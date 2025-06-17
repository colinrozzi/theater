## Description

The current Theater actor system processes all operations through a single handle interface, causing head-of-line blocking where slow operations (like network calls) prevent fast operations (like metrics queries) from being processed. This creates poor observability and control over running actors, especially during long-running operations or when actors become unresponsive.

This proposal introduces a **Split Handle Architecture** that separates actor execution from actor control and monitoring, enabling instant observability and safety controls while maintaining sequential execution semantics within each actor.

### Why This Change Is Necessary

- **Head-of-Line Blocking**: Currently, `get_metrics()` and `get_state()` calls are blocked by slow `call_function()` operations, making it impossible to monitor actors during execution
- **Safety Concerns**: No way to interrupt or shutdown actors that are stuck in infinite loops or long-running operations
- **Poor Observability**: Cannot get real-time status of actors during execution, making debugging and monitoring extremely difficult
- **Limited Control**: Shutdown and pause operations may not work if an actor is busy with a long operation

### Expected Benefits

- **Instant Observability**: Metrics, state, and status queries return in < 1ms regardless of ongoing operations
- **Safety Controls**: Ability to interrupt, pause, or force-shutdown unresponsive actors
- **Real-time Monitoring**: Continuous visibility into actor status during long-running operations
- **Better Debugging**: Instant access to actor state and chain events for debugging
- **Improved Reliability**: Watchdog systems can monitor and control actors effectively

### Potential Risks

- **Breaking Changes**: Complete API redesign requires updating all client code
- **Complexity**: Internal implementation becomes more complex with dual channels
- **Resource Usage**: Additional memory overhead for shared state and dual processing loops
- **State Consistency**: Need to carefully manage shared access to actor state

### Alternatives Considered

- **Three-tier operation processing** with interrupt operations (rejected as overly complex for current needs)
- **Work-stealing between actors** (rejected as it changes execution semantics unnecessarily)
- **Supervision tree architecture** (rejected as too large a change for this iteration)
- **Simple shared state reads** (rejected as it doesn't solve the control problem)

## Technical Approach

### 1. Split Handle Types

Replace single `ActorHandle` with two specialized handles:

```rust
// Handle for executing operations (potentially slow)
pub struct ActorExecutor {
    actor_id: TheaterId,
    execution_tx: mpsc::Sender<ExecutionOperation>,
}

// Handle for control and monitoring (always fast)
pub struct ActorController {
    actor_id: TheaterId,
    control_tx: mpsc::Sender<ControlOperation>,
    shared_metrics: Arc<RwLock<ActorMetrics>>,
    shared_state: Arc<RwLock<Option<Vec<u8>>>>,
    shared_chain: Arc<RwLock<Vec<ChainEvent>>>,
    state_updates: watch::Receiver<ActorState>,
}
```

### 2. Operation Classification

Split operations into two categories:

```rust
// Operations that modify actor state (sequential execution required)
pub enum ExecutionOperation {
    CallFunction { name: String, params: Vec<u8>, response_tx: oneshot::Sender<Result<Vec<u8>, ActorError>> },
    UpdateComponent { component_address: String, response_tx: oneshot::Sender<Result<(), ActorError>> },
    SaveChain { response_tx: oneshot::Sender<Result<(), ActorError>> },
}

// Operations for control and lifecycle (can interrupt execution)
pub enum ControlOperation {
    Pause { response_tx: oneshot::Sender<Result<(), ActorError>> },
    Resume { response_tx: oneshot::Sender<Result<(), ActorError>> },
    Shutdown { timeout: Option<Duration>, response_tx: oneshot::Sender<Result<(), ActorError>> },
    ForceStop { response_tx: oneshot::Sender<Result<(), ActorError>> },
}
```

### 3. Dual Processing Architecture

Implement separate processing loops:

1. **Execution Handler**: Processes `ExecutionOperation`s sequentially (like current implementation)
2. **Control Handler**: Processes `ControlOperation`s with ability to interrupt execution
3. **Shared State Management**: Real-time progressive updates to shared state accessible instantly by controller

### 4. State Consistency Model

**Progressive Updates**: Maintains the current chain event model where state is updated during operations:
- Chain events continue to be added as operations progress (current behavior)
- Shared state (`Arc<RwLock<T>>`) is updated in real-time during long-running operations
- `controller.metrics()` sees live updates, not just final results
- Consistency model remains the same as current implementation

### 5. Resource Management

**Internal Channel Management**: `ActorRuntime` manages both execution and control channels internally:
- `TheaterRuntime` returns user-facing `(ActorExecutor, ActorController)` handles
- `ActorRuntime` internally manages both `execution_tx` and `control_tx` channels
- Single shutdown signal in `ActorRuntime` closes both channels simultaneously
- When actor shuts down, both user handles become inert and return `ActorError::ChannelClosed`

### 6. Error Handling

**Interrupt Operations**: New error type for interrupted execution:
- Add `ActorError::Interrupted` for operations stopped by control commands
- Execution operations return `ActorError::Interrupted` when interrupted by `ForceStop`
- Control operations return their specific error types (e.g., `ActorError::ChannelClosed`)
- Failed execution operations with pending control operations return the execution error

### 7. API Changes

**Before (current)**:
```rust
let actor = theater.spawn_actor(manifest).await?;
let result = actor.call_function("slow_operation", params).await?; // Blocks everything
let metrics = actor.get_metrics().await?; // Blocked by slow_operation
```

**After (split handles)**:
```rust
let (executor, controller) = theater.spawn_actor(manifest).await?;
let result = executor.call_function("slow_operation", params).await?; // Can be slow
let metrics = controller.metrics(); // Always instant (no await!)
```

## Implementation Plan

### Phase 1: Core Infrastructure (Week 1)
- [ ] Add `ActorError::Interrupted` to error types
- [ ] Create `ActorExecutor` and `ActorController` types
- [ ] Add `ExecutionOperation` and `ControlOperation` enums
- [ ] Implement shared state management with `Arc<RwLock<T>>`
- [ ] Add real-time state broadcasting with `watch::channel`

### Phase 2: Runtime Integration (Week 2)
- [ ] Modify `TheaterRuntime::spawn_actor()` to return split handles
- [ ] Implement dual processing loops in `ActorRuntime`
- [ ] Add execution handler for sequential operation processing
- [ ] Add control handler with interrupt capabilities
- [ ] Update `ActorRuntime` to manage both channels internally
- [ ] Implement unified shutdown for both channels

### Phase 3: Safety and Control (Week 3)
- [ ] Implement pause/resume functionality
- [ ] Add graceful shutdown with timeout
- [ ] Add force-stop capability for unresponsive actors
- [ ] Implement interrupt mechanisms for long-running operations

### Phase 4: Testing and Documentation (Week 4)
- [ ] Comprehensive test suite for split handle behavior
- [ ] Performance benchmarks vs current implementation
- [ ] Update all documentation and examples
- [ ] Migration guide for existing Theater applications

## Breaking Changes

This is a **major breaking change** that affects all Theater users:

### API Changes
- `TheaterRuntime::spawn_actor()` now returns `(ActorExecutor, ActorController)` instead of `ActorHandle`
- `ActorHandle` is removed entirely
- All operations are now split between executor and controller

### Behavioral Changes
- Metrics and state access becomes synchronous (no `await` needed)
- Control operations can interrupt execution operations
- Actor state is accessible in real-time during operations

### Migration Required
All existing code using `ActorHandle` must be updated to use the new split handle API.

## Success Metrics

- [ ] Metrics queries respond in < 1ms regardless of ongoing operations
- [ ] Force shutdown completes in < 100ms even during infinite loops
- [ ] Real-time monitoring works during long-running operations
- [ ] Memory overhead < 10% increase per actor
- [ ] Execution performance unchanged for function calls

## Files Modified

### New Files
- `src/actor/executor.rs` - ActorExecutor implementation
- `src/actor/controller.rs` - ActorController implementation
- `src/actor/operations.rs` - Operation type definitions
- `src/actor/shared_state.rs` - Shared state management

### Modified Files
- `src/theater_runtime.rs` - Update spawn_actor API
- `src/actor/runtime.rs` - Implement dual processing loops and unified shutdown
- `src/actor/types.rs` - Add ActorError::Interrupted and update operation types
- `src/actor/handle.rs` - Remove old ActorHandle
- `src/messages.rs` - Update command types
- `examples/` - Update all examples
- `tests/` - Update all tests
- Documentation and README files

## Rollout Strategy

Since this is early in the project lifecycle:

1. **Immediate Implementation**: No backwards compatibility needed
2. **Update Examples**: All examples updated to use new API
3. **Documentation**: Complete rewrite of actor interaction documentation
4. **Testing**: Comprehensive test coverage for new architecture

This change fundamentally improves Theater's usability and safety while maintaining the core benefits of isolated actor execution.
