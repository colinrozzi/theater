// CONCEPT: Modified ActorRuntime::start to handle commands during startup

impl ActorRuntime {
    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        // ... other params
        info_rx: Receiver<ActorInfo>,
        control_rx: Receiver<ActorControl>,
        response_tx: Sender<StartActorResult>,
        // ... more params
    ) {
        // STEP 1: Return actor ID immediately
        let _ = response_tx.send(StartActorResult::Success(id.clone())).await;
        
        // STEP 2: Start a minimal command listener IMMEDIATELY
        let (startup_complete_tx, mut startup_complete_rx) = oneshot::channel::<ActorInstance>();
        let (startup_error_tx, mut startup_error_rx) = oneshot::channel::<ActorError>();
        
        // Clone channels for the startup listener
        let mut info_rx_startup = info_rx;
        let mut control_rx_startup = control_rx;
        let id_for_listener = id.clone();
        let theater_tx_for_listener = theater_tx.clone();
        
        // Spawn minimal command listener that runs DURING startup
        let startup_listener = tokio::spawn(async move {
            let mut startup_status = "Starting".to_string();
            let mut shutdown_requested = false;
            let mut shutdown_response: Option<oneshot::Sender<Result<(), ActorError>>> = None;
            
            loop {
                tokio::select! {
                    // Startup completed successfully
                    Ok(actor_instance) = &mut startup_complete_rx => {
                        info!("Startup completed, transitioning to main runtime");
                        return Ok((actor_instance, info_rx_startup, control_rx_startup, shutdown_requested, shutdown_response));
                    }
                    
                    // Startup failed
                    Ok(error) = &mut startup_error_rx => {
                        error!("Startup failed: {:?}", error);
                        
                        // Notify theater runtime of startup failure
                        let _ = theater_tx_for_listener.send(TheaterCommand::ActorError {
                            actor_id: id_for_listener.clone(),
                            error: error.clone(),
                        }).await;
                        
                        // Respond to any pending shutdown request
                        if let Some(response_tx) = shutdown_response {
                            let _ = response_tx.send(Err(error.clone()));
                        }
                        
                        return Err(error);
                    }
                    
                    // Handle info requests during startup
                    Some(info) = info_rx_startup.recv() => {
                        match info {
                            ActorInfo::GetStatus { response_tx } => {
                                let _ = response_tx.send(Ok(startup_status.clone()));
                            }
                            ActorInfo::GetState { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
                            }
                            ActorInfo::GetChain { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
                            }
                            ActorInfo::GetMetrics { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
                            }
                            ActorInfo::SaveChain { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
                            }
                        }
                    }
                    
                    // Handle control commands during startup  
                    Some(control) = control_rx_startup.recv() => {
                        match control {
                            ActorControl::Shutdown { response_tx } => {
                                info!("Shutdown requested during startup");
                                shutdown_requested = true;
                                shutdown_response = Some(response_tx);
                                startup_status = "Shutting down (startup)".to_string();
                                // Note: We'll handle the actual shutdown after startup completes or fails
                            }
                            ActorControl::Pause { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Cannot pause during startup".to_string())));
                            }
                            ActorControl::Resume { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Cannot resume during startup".to_string())));
                            }
                        }
                    }
                }
            }
        });
        
        // STEP 3: Run startup process in parallel
        let startup_process = tokio::spawn(async move {
            // Update status as we progress
            startup_status = "Loading manifest".to_string();
            
            // Setup actor store and manifest
            let (actor_store, _manifest_id) = match Self::setup_actor_store(
                id.clone(),
                theater_tx.clone(),
                actor_handle.clone(),
                config,
                &startup_error_tx, // Send errors to startup listener instead
            ).await {
                Ok(result) => result,
                Err(_) => return, // Error already sent
            };
            
            startup_status = "Validating permissions".to_string();
            
            // ... rest of startup process with status updates ...
            
            startup_status = "Instantiating component".to_string();
            
            // When everything is ready, send the completed instance
            let _ = startup_complete_tx.send(actor_instance);
        });
        
        // STEP 4: Wait for startup to complete and handle transition
        match startup_listener.await {
            Ok(Ok((actor_instance, info_rx, control_rx, shutdown_requested, shutdown_response))) => {
                if shutdown_requested {
                    // Handle shutdown that was requested during startup
                    info!("Processing shutdown request from startup phase");
                    if let Some(response_tx) = shutdown_response {
                        let _ = response_tx.send(Ok(()));
                    }
                    return; // Exit without starting main runtime
                }
                
                // STEP 5: Transition to main runtime with already-listening channels
                Self::run_communication_loops(
                    Arc::new(RwLock::new(actor_instance)),
                    operation_rx,  // Operation channel starts listening here
                    info_rx,       // Info channel transitions from startup listener
                    control_rx,    // Control channel transitions from startup listener
                    // ... other params
                ).await;
            }
            Ok(Err(startup_error)) => {
                // Startup failed, error already reported to theater runtime
                error!("Actor startup failed: {:?}", startup_error);
            }
            Err(join_error) => {
                error!("Startup listener task failed: {:?}", join_error);
            }
        }
    }
}