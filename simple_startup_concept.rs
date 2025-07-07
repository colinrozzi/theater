// SIMPLER APPROACH: Just handle critical commands during startup

impl ActorRuntime {
    pub async fn start(/* ... */) {
        // Return actor ID immediately
        let _ = response_tx.send(StartActorResult::Success(id.clone())).await;
        
        // Create a simple status tracker
        let startup_status = Arc::new(RwLock::new("Starting".to_string()));
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        
        // Spawn minimal listener for just status and shutdown
        let status_clone = startup_status.clone();
        let shutdown_clone = shutdown_flag.clone();
        let mut info_rx_clone = info_rx;
        let mut control_rx_clone = control_rx;
        
        let minimal_listener = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(ActorInfo::GetStatus { response_tx }) = info_rx_clone.recv() => {
                        let status = status_clone.read().await.clone();
                        let _ = response_tx.send(Ok(status));
                    }
                    Some(ActorControl::Shutdown { response_tx }) = control_rx_clone.recv() => {
                        shutdown_clone.store(true, Ordering::Relaxed);
                        let _ = response_tx.send(Ok(()));
                        break;
                    }
                    // Drop other commands during startup
                    Some(_) = info_rx_clone.recv() => {
                        // Ignore other info requests
                    }
                    Some(_) = control_rx_clone.recv() => {
                        // Ignore other control requests  
                    }
                }
            }
        });
        
        // Run startup with status updates
        *startup_status.write().await = "Loading manifest".to_string();
        // ... setup steps with status updates ...
        
        // Check for shutdown periodically
        if shutdown_flag.load(Ordering::Relaxed) {
            minimal_listener.abort();
            return; // Exit early
        }
        
        *startup_status.write().await = "Ready".to_string();
        minimal_listener.abort();
        
        // Start main runtime
        Self::run_communication_loops(/* ... */).await;
    }
}