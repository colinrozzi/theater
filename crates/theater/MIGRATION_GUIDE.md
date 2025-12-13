# Step-by-Step Migration Guide

This guide shows how to migrate from the current implementation to the explicit state machine **incrementally**, without breaking existing functionality.

## Phase 1: Define Types (No Behavior Changes)

### Step 1.1: Add the state enum alongside existing code

```rust
// In actor/runtime.rs - ADD this, don't replace anything yet

/// New explicit state enum - will gradually migrate to this
#[allow(dead_code)] // Remove this as we migrate
enum ActorState {
    Starting {
        setup_task: JoinHandle<Result<SetupComplete, ActorError>>,
        status_rx: Receiver<String>,
        current_status: String,
        pending_shutdown: Option<oneshot::Sender<Result<(), ActorError>>>,
    },
    Idle {
        resources: ActorResources,
    },
    Processing {
        resources: ActorResources,
        current_operation: JoinHandle<Result<Vec<u8>, ActorError>>,
        operation_name: String,
        pending_shutdown: Option<oneshot::Sender<Result<(), ActorError>>>,
    },
    Paused {
        resources: ActorResources,
    },
    ShuttingDown,
}

struct ActorResources {
    instance: Arc<RwLock<ActorInstance>>,
    metrics: Arc<RwLock<MetricsCollector>>,
    handler_tasks: Vec<JoinHandle<()>>,
    shutdown_controller: ShutdownController,
}

struct SetupComplete {
    instance: ActorInstance,
    handler_tasks: Vec<JoinHandle<()>>,
    metrics: MetricsCollector,
}

enum StateTransition {
    Continue(ActorState),
    Shutdown,
    Error(ActorError),
}
```

**Test:** Code still compiles ✅

### Step 1.2: Create conversion helpers

```rust
impl ActorRuntime {
    /// Helper to convert from old-style state to new enum
    #[allow(dead_code)]
    fn to_state_enum(
        actor_instance: &Option<Arc<RwLock<ActorInstance>>>,
        metrics: &Option<Arc<RwLock<MetricsCollector>>>,
        handler_tasks: &[JoinHandle<()>],
        current_operation: &Option<JoinHandle<()>>,
        paused: bool,
        shutdown_requested: bool,
    ) -> Option<ActorState> {
        if shutdown_requested {
            return Some(ActorState::ShuttingDown);
        }

        if let (Some(instance), Some(metrics)) = (actor_instance, metrics) {
            let resources = ActorResources {
                instance: instance.clone(),
                metrics: metrics.clone(),
                handler_tasks: handler_tasks.to_vec(),
                shutdown_controller: ShutdownController::new(),
            };

            if paused {
                return Some(ActorState::Paused { resources });
            } else if current_operation.is_some() {
                // We'd need more info for Processing, so return None for now
                return None;
            } else {
                return Some(ActorState::Idle { resources });
            }
        }

        None // Still starting
    }
}
```

**Test:** Code still compiles, no behavior changes ✅

## Phase 2: Extract One State Handler (First Real Change)

### Step 2.1: Extract the Paused state handler

This is the **simplest state** - good place to start!

```rust
impl ActorRuntime {
    /// New handler for Paused state
    async fn handle_paused_state_new(
        &mut self,
        resources: ActorResources,
        // Keep receiving from original channels
        info_rx: &mut Receiver<ActorInfo>,
        control_rx: &mut Receiver<ActorControl>,
        parent_shutdown_receiver: &mut ShutdownReceiver,
        operation_rx: &mut Receiver<ActorOperation>,
    ) -> StateTransition {
        tokio::select! {
            Some(_op) = operation_rx.recv() => {
                // This shouldn't happen, but if it does, ignore it
                error!("Operation received while paused");
                StateTransition::Continue(ActorState::Paused { resources })
            }

            Some(info) = info_rx.recv() => {
                self.handle_info_paused(&resources, info).await;
                StateTransition::Continue(ActorState::Paused { resources })
            }

            Some(control) = control_rx.recv() => {
                match control {
                    ActorControl::Shutdown { response_tx } => {
                        info!("Shutdown requested while paused");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Terminate { response_tx } => {
                        info!("Terminate requested while paused");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Pause { response_tx } => {
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Continue(ActorState::Paused { resources })
                    }
                    ActorControl::Resume { response_tx } => {
                        info!("Resuming actor");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Continue(ActorState::Idle { resources })
                    }
                }
            }

            shutdown_signal = &mut parent_shutdown_receiver.receiver => {
                match shutdown_signal {
                    Ok(_) => StateTransition::Shutdown,
                    Err(e) => {
                        error!("Shutdown signal error: {:?}", e);
                        StateTransition::Shutdown
                    }
                }
            }
        }
    }

    async fn handle_info_paused(&self, resources: &ActorResources, info: ActorInfo) {
        match info {
            ActorInfo::GetStatus { response_tx } => {
                let _ = response_tx.send(Ok("Paused".to_string()));
            }
            ActorInfo::GetState { response_tx } => {
                let instance = resources.instance.read().await;
                let state = instance.store.data().get_state();
                let _ = response_tx.send(Ok(state));
            }
            ActorInfo::GetChain { response_tx } => {
                let instance = resources.instance.read().await;
                let chain = instance.store.data().get_chain();
                let _ = response_tx.send(Ok(chain));
            }
            ActorInfo::GetMetrics { response_tx } => {
                let metrics = resources.metrics.read().await;
                let metrics_data = metrics.get_metrics().await;
                let _ = response_tx.send(Ok(metrics_data));
            }
            ActorInfo::SaveChain { response_tx } => {
                let instance = resources.instance.read().await;
                match instance.save_chain() {
                    Ok(_) => { let _ = response_tx.send(Ok(())); }
                    Err(e) => { let _ = response_tx.send(Err(ActorError::UnexpectedError(e.to_string()))); }
                }
            }
        }
    }
}
```

### Step 2.2: Integrate into existing code

In your main `start()` loop, add this **at the very top** of the select block:

```rust
loop {
    // NEW: Check if we're in paused state and use new handler
    if *paused.read().await && actor_instance.is_some() && current_operation.is_none() {
        if let Some(resources) = Self::build_resources(
            &actor_instance,
            &metrics,
            &handler_tasks,
            &shutdown_controller,
        ) {
            info!("Using new paused state handler");
            match Self::handle_paused_state_new(
                &mut self,
                resources,
                &mut info_rx,
                &mut control_rx,
                &mut parent_shutdown_receiver,
                &mut operation_rx,
            ).await {
                StateTransition::Continue(ActorState::Idle { resources }) => {
                    // Unpack resources back into old variables
                    actor_instance = Some(resources.instance);
                    metrics = Some(resources.metrics);
                    handler_tasks = resources.handler_tasks;
                    *paused.write().await = false;
                    continue;
                }
                StateTransition::Continue(ActorState::Paused { .. }) => {
                    // Stay paused
                    continue;
                }
                StateTransition::Shutdown | StateTransition::Error(_) => {
                    break;
                }
                _ => unreachable!(),
            }
        }
    }

    // EXISTING: Original select block
    tokio::select! {
        // ... all your existing code ...
    }
}
```

**Test:** 
- Run all existing tests ✅
- Specifically test pausing/resuming ✅
- Add new unit tests for `handle_paused_state_new` ✅

### Step 2.3: Remove old paused handling

Once you've verified the new handler works, you can remove the old pause/resume logic from the main select block.

## Phase 3: Extract Idle State

This is the next most straightforward state.

```rust
impl ActorRuntime {
    async fn handle_idle_state_new(
        &mut self,
        resources: ActorResources,
        info_rx: &mut Receiver<ActorInfo>,
        control_rx: &mut Receiver<ActorControl>,
        operation_rx: &mut Receiver<ActorOperation>,
        parent_shutdown_receiver: &mut ShutdownReceiver,
        theater_tx: &Sender<TheaterCommand>,
    ) -> StateTransition {
        tokio::select! {
            Some(op) = operation_rx.recv() => {
                match op {
                    ActorOperation::CallFunction { name, params, response_tx } => {
                        info!("Starting operation: {}", name);
                        
                        let operation_task = self.spawn_operation(
                            &resources,
                            name.clone(),
                            params,
                            response_tx,
                            theater_tx,
                        );

                        StateTransition::Continue(ActorState::Processing {
                            resources,
                            current_operation: operation_task,
                            operation_name: name,
                            pending_shutdown: None,
                        })
                    }
                    ActorOperation::UpdateComponent { component_address: _, response_tx } => {
                        let _ = response_tx.send(Err(ActorError::UpdateComponentError(
                            "Not implemented".to_string()
                        )));
                        StateTransition::Continue(ActorState::Idle { resources })
                    }
                }
            }

            Some(info) = info_rx.recv() => {
                self.handle_info_idle(&resources, info).await;
                StateTransition::Continue(ActorState::Idle { resources })
            }

            Some(control) = control_rx.recv() => {
                match control {
                    ActorControl::Shutdown { response_tx } => {
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Terminate { response_tx } => {
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Pause { response_tx } => {
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Continue(ActorState::Paused { resources })
                    }
                    ActorControl::Resume { response_tx } => {
                        let _ = response_tx.send(Err(ActorError::NotPaused));
                        StateTransition::Continue(ActorState::Idle { resources })
                    }
                }
            }

            shutdown_signal = &mut parent_shutdown_receiver.receiver => {
                match shutdown_signal {
                    Ok(_) => StateTransition::Shutdown,
                    Err(e) => {
                        error!("Shutdown signal error: {:?}", e);
                        StateTransition::Shutdown
                    }
                }
            }
        }
    }
}
```

## Phase 4: Extract Processing State

This is more complex because of operation tracking.

```rust
async fn handle_processing_state_new(
    &mut self,
    resources: ActorResources,
    mut current_operation: JoinHandle<Result<Vec<u8>, ActorError>>,
    operation_name: String,
    pending_shutdown: Option<oneshot::Sender<Result<(), ActorError>>>,
    info_rx: &mut Receiver<ActorInfo>,
    control_rx: &mut Receiver<ActorControl>,
    parent_shutdown_receiver: &mut ShutdownReceiver,
    operation_rx: &mut Receiver<ActorOperation>,
) -> StateTransition {
    tokio::select! {
        result = &mut current_operation => {
            info!("Operation '{}' completed: {:?}", operation_name, result);
            
            if let Some(response_tx) = pending_shutdown {
                let _ = response_tx.send(Ok(()));
                return StateTransition::Shutdown;
            }

            StateTransition::Continue(ActorState::Idle { resources })
        }

        Some(info) = info_rx.recv() => {
            self.handle_info_processing(&resources, info).await;
            StateTransition::Continue(ActorState::Processing {
                resources,
                current_operation,
                operation_name,
                pending_shutdown,
            })
        }

        Some(control) = control_rx.recv() => {
            match control {
                ActorControl::Shutdown { response_tx } => {
                    info!("Shutdown requested - waiting for operation to complete");
                    StateTransition::Continue(ActorState::Processing {
                        resources,
                        current_operation,
                        operation_name,
                        pending_shutdown: Some(response_tx),
                    })
                }
                ActorControl::Terminate { response_tx } => {
                    info!("Terminate requested - aborting operation");
                    current_operation.abort();
                    let _ = response_tx.send(Ok(()));
                    StateTransition::Shutdown
                }
                ActorControl::Pause { response_tx } => {
                    let _ = response_tx.send(Err(ActorError::UnexpectedError(
                        "Cannot pause during operation".to_string()
                    )));
                    StateTransition::Continue(ActorState::Processing {
                        resources,
                        current_operation,
                        operation_name,
                        pending_shutdown,
                    })
                }
                ActorControl::Resume { response_tx } => {
                    let _ = response_tx.send(Err(ActorError::NotPaused));
                    StateTransition::Continue(ActorState::Processing {
                        resources,
                        current_operation,
                        operation_name,
                        pending_shutdown,
                    })
                }
            }
        }

        shutdown_signal = &mut parent_shutdown_receiver.receiver => {
            match shutdown_signal {
                Ok(signal) => {
                    match signal.shutdown_type {
                        ShutdownType::Graceful => {
                            info!("Graceful shutdown - waiting for operation");
                            StateTransition::Continue(ActorState::Processing {
                                resources,
                                current_operation,
                                operation_name,
                                pending_shutdown,
                            })
                        }
                        ShutdownType::Force => {
                            current_operation.abort();
                            StateTransition::Shutdown
                        }
                    }
                }
                Err(e) => {
                    error!("Shutdown signal error: {:?}", e);
                    StateTransition::Shutdown
                }
            }
        }

        // Ignore new operations while processing
        Some(_) = operation_rx.recv() => {
            error!("Operation received while processing - this shouldn't happen");
            StateTransition::Continue(ActorState::Processing {
                resources,
                current_operation,
                operation_name,
                pending_shutdown,
            })
        }
    }
}
```

## Phase 5: Extract Starting State

This is the most complex, save it for last.

## Phase 6: Replace Main Loop

Once all state handlers are extracted and tested, replace the main loop:

```rust
pub async fn start(...) {
    let mut state = ActorState::Starting { /* ... */ };

    loop {
        let transition = match state {
            ActorState::Starting { .. } => self.handle_starting_state().await,
            ActorState::Idle { .. } => self.handle_idle_state().await,
            ActorState::Processing { .. } => self.handle_processing_state().await,
            ActorState::Paused { .. } => self.handle_paused_state().await,
            ActorState::ShuttingDown => break,
        };

        match transition {
            StateTransition::Continue(new_state) => {
                state = new_state;
            }
            StateTransition::Shutdown => {
                self.transition_to_shutdown(state).await;
                break;
            }
            StateTransition::Error(error) => {
                self.notify_error(error).await;
                self.transition_to_shutdown(state).await;
                break;
            }
        }
    }
}
```

## Testing Strategy

For each phase:

1. **Write tests first** for the new state handler
2. **Run existing integration tests** to ensure no regressions
3. **Add logging** to track state transitions
4. **Run in development** for a period before moving to next phase

### Example Test

```rust
#[tokio::test]
async fn test_pause_from_idle() {
    let (mut runtime, channels) = create_test_runtime();
    
    // Set up idle state
    let resources = create_test_resources();
    runtime.state = ActorState::Idle { resources };
    
    // Send pause command
    let (response_tx, response_rx) = oneshot::channel();
    channels.control_tx.send(ActorControl::Pause { response_tx }).await.unwrap();
    
    // Handle state
    let transition = runtime.handle_idle_state_new(/* ... */).await;
    
    // Verify transition to paused
    assert!(matches!(transition, StateTransition::Continue(ActorState::Paused { .. })));
    
    // Verify response
    assert!(response_rx.await.unwrap().is_ok());
}
```

## Rollback Plan

At any phase, if issues arise:

1. Comment out the new handler integration
2. The old code path still works
3. Fix the issue
4. Re-enable the new handler

This is the beauty of incremental migration!

## Timeline Estimate

- **Phase 1 (Types):** 1-2 hours
- **Phase 2 (Paused):** 4-6 hours (includes testing)
- **Phase 3 (Idle):** 4-6 hours
- **Phase 4 (Processing):** 6-8 hours (most complex)
- **Phase 5 (Starting):** 6-8 hours
- **Phase 6 (Replace main loop):** 2-4 hours
- **Total:** ~25-35 hours spread over 1-2 weeks

But you get incremental benefits after each phase!

## Success Metrics

You'll know the refactoring is working when:

- ✅ New code has fewer lines but is more readable
- ✅ State transitions are logged clearly
- ✅ Tests are easier to write
- ✅ Bugs in state handling decrease
- ✅ New developers can understand the code faster
- ✅ Adding new states is straightforward

Good luck! 🚀
