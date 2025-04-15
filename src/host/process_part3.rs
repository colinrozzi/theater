impl ProcessHost {
    /// Set up WebAssembly host functions for the process interface
    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up host functions for process handling");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/process")
            .expect("Could not instantiate ntwk:theater/process");

        // Implementation for os-spawn
        let processes = self.processes.clone();
        let next_process_id = self.next_process_id.clone();
        let config = self.config.clone();
        interface.func_wrap_async(
            "os-spawn",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (process_config_wit,)|  // Process configuration from WebAssembly
                  -> Box<dyn Future<Output = Result<(Result<u64, String>,)>> + Send> {
                let processes = processes.clone();
                let next_process_id = next_process_id.clone();
                let config = config.clone();
                
                Box::new(async move {
                    // Parse wit parameters into native types
                    let process_config = parse_process_config(process_config_wit);
                    
                    let program = process_config.program.clone();
                    let args = process_config.args.clone();
                    let cwd = process_config.cwd.clone();
                    
                    // Validate program path
                    if let Some(allowed_programs) = &config.allowed_programs {
                        if !allowed_programs.contains(&program) {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: None,
                                    operation: "spawn".to_string(),
                                    message: format!("Program not allowed: {}", program),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to spawn process: Program not allowed: {}", program)),
                            });
                            
                            return Ok((Err(format!("Program not allowed: {}", program)),));
                        }
                    }
                    
                    // Validate working directory
                    if let Some(cwd_path) = &cwd {
                        if let Some(allowed_paths) = &config.allowed_paths {
                            if !allowed_paths.iter().any(|path| cwd_path.starts_with(path)) {
                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/spawn".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: None,
                                        operation: "spawn".to_string(),
                                        message: format!("Working directory not allowed: {}", cwd_path),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to spawn process: Working directory not allowed: {}", cwd_path)),
                                });
                                
                                return Ok((Err(format!("Working directory not allowed: {}", cwd_path)),));
                            }
                        }
                    }
                    
                    // Check if we've reached the max number of processes
                    {
                        let processes_lock = processes.lock().unwrap();
                        if processes_lock.len() >= config.max_processes {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: None,
                                    operation: "spawn".to_string(),
                                    message: "Too many processes".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some("Failed to spawn process: Too many processes running".to_string()),
                            });
                            
                            return Ok((Err("Too many processes running".to_string()),));
                        }
                    }
                    
                    // Get a new process ID
                    let process_id = {
                        let mut id_lock = next_process_id.lock().unwrap();
                        let id = *id_lock;
                        *id_lock += 1;
                        id
                    };
                    
                    // Start the process
                    let mut command = Command::new(&program);
                    command.args(&args);
                    
                    if let Some(cwd_path) = &cwd {
                        command.current_dir(cwd_path);
                    }
                    
                    for (key, value) in &process_config.env {
                        command.env(key, value);
                    }
                    
                    // Set up pipes for stdin, stdout, stderr
                    command.stdin(std::process::Stdio::piped());
                    command.stdout(std::process::Stdio::piped());
                    command.stderr(std::process::Stdio::piped());
                    
                    // Spawn the process
                    match command.spawn() {
                        Ok(mut child) => {
                            let os_pid = child.id();
                            
                            let actor_id = ctx.data().id.clone();
                            let theater_tx = ctx.data().theater_tx.clone();
                            
                            // Set up stdin channel
                            let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(32);
                            let mut stdin = child.stdin.take().expect("Failed to open stdin");
                            
                            // Handle stdin writes in a separate task
                            let stdin_writer = tokio::spawn(async move {
                                while let Some(data) = stdin_rx.recv().await {
                                    if let Err(e) = stdin.write_all(&data).await {
                                        error!("Failed to write to process stdin: {}", e);
                                        break;
                                    }
                                    if let Err(e) = stdin.flush().await {
                                        error!("Failed to flush process stdin: {}", e);
                                        break;
                                    }
                                }
                            });
                            
                            // Set up stdout reader
                            let stdout_handle = if let Some(stdout) = child.stdout.take() {
                                let stdout_mode = process_config.stdout_mode;
                                let buffer_size = process_config.buffer_size as usize;
                                let process_id_clone = process_id;
                                let actor_id_clone = actor_id.clone();
                                let theater_tx_clone = theater_tx.clone();
                                
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stdout,
                                        stdout_mode,
                                        buffer_size,
                                        process_id_clone,
                                        actor_id_clone,
                                        theater_tx_clone,
                                        true, // is stdout
                                    ).await;
                                }))
                            } else {
                                None
                            };
                            
                            // Set up stderr reader
                            let stderr_handle = if let Some(stderr) = child.stderr.take() {
                                let stderr_mode = process_config.stderr_mode;
                                let buffer_size = process_config.buffer_size as usize;
                                let process_id_clone = process_id;
                                let actor_id_clone = actor_id.clone();
                                let theater_tx_clone = theater_tx.clone();
                                
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stderr,
                                        stderr_mode,
                                        buffer_size,
                                        process_id_clone,
                                        actor_id_clone,
                                        theater_tx_clone,
                                        false, // not stdout (stderr)
                                    ).await;
                                }))
                            } else {
                                None
                            };
                            
                            // Capture the child for waiting
                            let mut wait_child = child.clone();
                            
                            // Create separate task for monitoring
                            let processes_clone = processes.clone();
                            let process_id_clone = process_id;
                            let actor_id_clone = actor_id.clone();
                            let theater_tx_clone = theater_tx.clone();
                            
                            // Spawn a task to wait for the process to exit
                            tokio::spawn(async move {
                                if let Ok(status) = wait_child.wait().await {
                                    let exit_code = status.code().unwrap_or(-1);
                                    
                                    // Create the event data
                                    let event_data = ChainEventData {
                                        event_type: "process/exit".to_string(),
                                        data: EventData::Process(ProcessEventData::ProcessExit {
                                            process_id: process_id_clone,
                                            exit_code,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Process {} exited with code {}", process_id_clone, exit_code)),
                                    };
                                    
                                    // Send exit event to the actor
                                    let _ = theater_tx_clone.send(crate::messages::TheaterCommand::NewEvent {
                                        actor_id: actor_id_clone,
                                        event: event_data.to_chain_event(None),
                                    }).await;
                                    
                                    // Update process status
                                    let mut processes = processes_clone.lock().unwrap();
                                    if let Some(process) = processes.get_mut(&process_id_clone) {
                                        process.exit_code = Some(exit_code);
                                        process.child = None;
                                    }
                                }
                            });
                            
                            // Create the managed process
                            let managed_process = ManagedProcess {
                                id: process_id,
                                child: Some(child),
                                os_pid: Some(os_pid),
                                config: process_config.clone(),
                                start_time: SystemTime::now(),
                                stdin_writer: Some(stdin_writer),
                                stdin_tx: Some(stdin_tx),
                                stdout_reader: stdout_handle,
                                stderr_reader: stderr_handle,
                                exit_code: None,
                            };
                            
                            // Store the managed process
                            {
                                let mut processes = processes.lock().unwrap();
                                processes.insert(process_id, managed_process);
                            }
                            
                            // Record spawn event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
                                data: EventData::Process(ProcessEventData::ProcessSpawn {
                                    process_id,
                                    program: program.clone(),
                                    args: args.clone(),
                                    os_pid: Some(os_pid),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Spawned process {} (OS PID: {:?}): {}", process_id, os_pid, program)),
                            });
                            
                            Ok((Ok(process_id),))
                        },
                        Err(e) => {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: None,
                                    operation: "spawn".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to spawn process: {}", e)),
                            });
                            
                            Ok((Err(format!("Failed to spawn process: {}", e)),))
                        }
                    }
                })
            },
        )?;

        // Implementation for os-write-stdin
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-write-stdin",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid, data): (u64, Vec<u8>)|
                  -> Box<dyn Future<Output = Result<(Result<u32, String>,)>> + Send> {
                let processes = processes.clone();
                
                Box::new(async move {
                    let stdin_tx = {
                        // We need to drop the mutex guard before the async operation
                        let processes = processes.lock().unwrap();
                        
                        if let Some(process) = processes.get(&pid) {
                            if let Some(tx) = &process.stdin_tx {
                                Some(tx.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    
                    if let Some(stdin_tx) = stdin_tx {
                        // Send data to the stdin writer
                        let bytes_written = data.len() as u32;
                        match stdin_tx.send(data).await {
                            Ok(_) => {
                                // Record stdin write event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/write-stdin".to_string(),
                                    data: EventData::Process(ProcessEventData::StdinWrite {
                                        process_id: pid,
                                        bytes_written,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Wrote {} bytes to process {} stdin", bytes_written, pid)),
                                });
                                
                                Ok((Ok(bytes_written),))
                            },
                            Err(e) => {
                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/write-stdin".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: Some(pid),
                                        operation: "write-stdin".to_string(),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to write to process {} stdin: {}", pid, e)),
                                });
                                
                                Ok((Err(format!("Failed to write to stdin: {}", e)),))
                            }
                        }
                    } else {
                        // Process not found or stdin not available
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/write-stdin".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "write-stdin".to_string(),
                                message: "Process stdin not available".to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Failed to write to process {}: stdin not available", pid)),
                        });
                        
                        Ok((Err("Process stdin not available".to_string()),))
                    }
                })
            },
        )?;

        // Implementation for os-status
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-status",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<ProcessStatus, String>,)>> + Send> {
                let processes = processes.clone();
                
                Box::new(async move {
                    let process_status = {
                        let processes = processes.lock().unwrap();
                        
                        if let Some(process) = processes.get(&pid) {
                            let start_time = match process.start_time.duration_since(std::time::UNIX_EPOCH) {
                                Ok(duration) => duration.as_millis() as u64,
                                Err(_) => 0,
                            };
                            
                            Some(ProcessStatus {
                                pid,
                                running: process.child.is_some(),
                                exit_code: process.exit_code,
                                start_time,
                                cpu_usage: 0.0, // Not implemented yet
                                memory_usage: 0, // Not implemented yet
                            })
                        } else {
                            None
                        }
                    };
                    
                    if let Some(status) = process_status {
                        Ok((Ok(status),))
                    } else {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/status".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "status".to_string(),
                                message: format!("Process not found: {}", pid),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Failed to get status for process {}: process not found", pid)),
                        });
                        
                        Ok((Err(format!("Process not found: {}", pid)),))
                    }
                })
            },
        )?;

        // Implementation for os-kill
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-kill",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let processes = processes.clone();
                
                Box::new(async move {
                    // Get a clone of the child process to kill
                    let child_opt = {
                        let mut processes = processes.lock().unwrap();
                        if let Some(process) = processes.get_mut(&pid) {
                            process.child.take()
                        } else {
                            None
                        }
                    };
                    
                    // Kill the process if we have a handle
                    if let Some(mut child) = child_opt {
                        match child.kill().await {
                            Ok(_) => {
                                // Record kill event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/kill".to_string(),
                                    data: EventData::Process(ProcessEventData::KillRequest {
                                        process_id: pid,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Killed process {}", pid)),
                                });
                                
                                // Update the process status to reflect it's been killed
                                {
                                    let mut processes = processes.lock().unwrap();
                                    if let Some(process) = processes.get_mut(&pid) {
                                        process.exit_code = Some(-1); // Killed
                                    }
                                }
                                
                                Ok((Ok(()),))
                            },
                            Err(e) => {
                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/kill".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: Some(pid),
                                        operation: "kill".to_string(),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to kill process {}: {}", pid, e)),
                                });
                                
                                Ok((Err(format!("Failed to kill process: {}", e)),))
                            }
                        }
                    } else {
                        // Process is already dead, so consider this a success
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/kill".to_string(),
                            data: EventData::Process(ProcessEventData::KillRequest {
                                process_id: pid,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Process {} is already terminated", pid)),
                        });
                        
                        Ok((Ok(()),))
                    }
                })
            },
        )?;
        
        // Implementation for os-signal
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-signal",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid, signal): (u64, u32)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let processes = processes.clone();
                
                Box::new(async move {
                    // Get the process OS PID
                    let os_pid = {
                        let processes = processes.lock().unwrap();
                        if let Some(process) = processes.get(&pid) {
                            if let Some(child) = &process.child {
                                child.id()
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    
                    if let Some(os_pid) = os_pid {
                        #[cfg(unix)]
                        {
                            // Map signal numbers to Unix signals
                            let sig = match signal {
                                1 => 1,  // SIGHUP
                                2 => 2,  // SIGINT
                                15 => 15, // SIGTERM
                                _ => signal as i32
                            };
                            
                            // Send the signal
                            unsafe {
                                libc::kill(os_pid as i32, sig);
                            }
                            
                            // Record signal event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/signal".to_string(),
                                data: EventData::Process(ProcessEventData::SignalSent {
                                    process_id: pid,
                                    signal,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Sent signal {} to process {}", signal, pid)),
                            });
                            
                            Ok((Ok(()),))
                        }
                        
                        #[cfg(not(unix))]
                        {
                            // On non-Unix platforms, we can only kill the process
                            if signal == 15 { // SIGTERM equivalent
                                let mut processes = processes.lock().unwrap();
                                if let Some(process) = processes.get_mut(&pid) {
                                    if let Some(child) = &mut process.child {
                                        let _ = child.kill();
                                    }
                                }
                                
                                // Record signal event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/signal".to_string(),
                                    data: EventData::Process(ProcessEventData::SignalSent {
                                        process_id: pid,
                                        signal,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Sent kill signal to process {}", pid)),
                                });
                                
                                Ok((Ok(()),))
                            } else {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/signal".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: Some(pid),
                                        operation: "signal".to_string(),
                                        message: format!("Signal {} not supported on this platform", signal),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Signal {} not supported on this platform", signal)),
                                });
                                
                                Ok((Err(format!("Signal {} not supported on this platform", signal)),))
                            }
                        }
                    } else {
                        // Process not found or not running
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/signal".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "signal".to_string(),
                                message: "Process not found or not running".to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Failed to send signal to process {}: process not found or not running", pid)),
                        });
                        
                        Ok((Err("Process not found or not running".to_string()),))
                    }
                })
            },
        )?;
        
        Ok(())
    }
}
