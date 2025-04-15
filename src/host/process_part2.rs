impl ProcessHost {
    /// Create a new ProcessHost with the given configuration
    pub fn new(config: ProcessHostConfig) -> Self {
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
            next_process_id: Arc::new(Mutex::new(1)),
            actor_handle: None,
        }
    }

    /// Start the process host
    pub async fn start(
        &mut self,
        actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting ProcessHost");
        self.actor_handle = Some(actor_handle);
        Ok(())
    }

    /// Add export functions to the actor instance
    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding export functions for process handling");

        // Register the process handler export functions
        actor_instance
            .register_function_no_result::<(u64, Vec<u8>)>(
                "process",
                "handle-stdout",
            )
            .expect("Failed to register handle-stdout function");
            
        actor_instance
            .register_function_no_result::<(u64, Vec<u8>)>(
                "process",
                "handle-stderr",
            )
            .expect("Failed to register handle-stderr function");
            
        actor_instance
            .register_function_no_result::<(u64, i32)>(
                "process",
                "handle-exit",
            )
            .expect("Failed to register handle-exit function");

        Ok(())
    }

    /// Process output from a child process
    async fn process_output<R>(
        mut reader: R,
        mode: OutputMode,
        buffer_size: usize,
        process_id: u64,
        actor_id: crate::id::TheaterId,
        theater_tx: tokio::sync::mpsc::Sender<crate::messages::TheaterCommand>,
        is_stdout: bool,
    ) where
        R: AsyncReadExt + Unpin + Send + 'static,
    {
        match mode {
            OutputMode::Raw => {
                // Read in raw mode without any special processing
                let mut buffer = vec![0; buffer_size];
                loop {
                    match reader.read(&mut buffer).await {
                        Ok(n) if n > 0 => {
                            // Create event data
                            let event_data = if is_stdout {
                                EventData::Process(ProcessEventData::StdoutOutput {
                                    process_id,
                                    bytes: n,
                                })
                            } else {
                                EventData::Process(ProcessEventData::StderrOutput {
                                    process_id,
                                    bytes: n,
                                })
                            };
                            
                            // Send the output to the actor
                            let data = buffer[0..n].to_vec();
                            let _ = theater_tx
                                .send(crate::messages::TheaterCommand::SendMessage {
                                    actor_id: actor_id.clone(),
                                    actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                        data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                    }),
                                })
                                .await;
                        },
                        Ok(_) => break, // EOF
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            },
            OutputMode::LineByLine => {
                // Process output line by line
                let mut line = vec![];
                let mut buffer = vec![0; 1]; // Read one byte at a time for line processing
                
                loop {
                    match reader.read(&mut buffer).await {
                        Ok(n) if n > 0 => {
                            if buffer[0] == b'\n' {
                                // Line complete, send it
                                if !line.is_empty() {
                                    // Send the line to the actor
                                    let data = line.clone();
                                    let _ = theater_tx
                                        .send(crate::messages::TheaterCommand::SendMessage {
                                            actor_id: actor_id.clone(),
                                            actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                                data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                            }),
                                        })
                                        .await;
                                    
                                    line.clear();
                                }
                            } else {
                                line.push(buffer[0]);
                                
                                // Check if line is too long
                                if line.len() >= buffer_size {
                                    // Send the partial line to the actor
                                    let data = line.clone();
                                    let _ = theater_tx
                                        .send(crate::messages::TheaterCommand::SendMessage {
                                            actor_id: actor_id.clone(),
                                            actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                                data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                            }),
                                        })
                                        .await;
                                    
                                    line.clear();
                                }
                            }
                        },
                        Ok(_) => {
                            // EOF - send any remaining data
                            if !line.is_empty() {
                                // Send the line to the actor
                                let data = line.clone();
                                let _ = theater_tx
                                    .send(crate::messages::TheaterCommand::SendMessage {
                                        actor_id: actor_id.clone(),
                                        actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                            data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                        }),
                                    })
                                    .await;
                            }
                            break;
                        },
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            },
            OutputMode::Json => {
                // Process output as JSON objects (newline-delimited)
                let mut buffer = String::new();
                let mut temp_buffer = vec![0; 1024]; // Read in chunks
                
                loop {
                    match reader.read(&mut temp_buffer).await {
                        Ok(n) if n > 0 => {
                            let chunk = String::from_utf8_lossy(&temp_buffer[0..n]);
                            buffer.push_str(&chunk);
                            
                            // Process complete JSON objects
                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[0..pos].trim().to_string();
                                let remaining = buffer[pos+1..].to_string();
                                buffer = remaining;
                                
                                if !line.is_empty() {
                                    // Validate JSON
                                    if let Ok(_) = serde_json::from_str::<serde_json::Value>(&line) {
                                        // Valid JSON, send it
                                        let data = line.as_bytes().to_vec();
                                        let _ = theater_tx
                                            .send(crate::messages::TheaterCommand::SendMessage {
                                                actor_id: actor_id.clone(),
                                                actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                                    data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                                }),
                                            })
                                            .await;
                                    }
                                }
                            }
                            
                            // Check if buffer is too large
                            if buffer.len() > buffer_size {
                                // Buffer too large, flush it as raw data
                                let data = buffer.as_bytes().to_vec();
                                let _ = theater_tx
                                    .send(crate::messages::TheaterCommand::SendMessage {
                                        actor_id: actor_id.clone(),
                                        actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                            data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                        }),
                                    })
                                    .await;
                                
                                buffer.clear();
                            }
                        },
                        Ok(_) => {
                            // EOF - send any remaining data
                            if !buffer.is_empty() {
                                let data = buffer.as_bytes().to_vec();
                                let _ = theater_tx
                                    .send(crate::messages::TheaterCommand::SendMessage {
                                        actor_id: actor_id.clone(),
                                        actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                            data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                        }),
                                    })
                                    .await;
                            }
                            break;
                        },
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            },
            OutputMode::Chunked => {
                // Read in fixed-size chunks
                let chunk_size = match mode {
                    OutputMode::Chunked => buffer_size,
                    _ => unreachable!(), // We're in the Chunked match arm
                };
                
                let mut buffer = vec![0; chunk_size];
                
                loop {
                    match reader.read_exact(&mut buffer).await {
                        Ok(_) => {
                            // Send the chunk to the actor
                            let data = buffer.clone();
                            let _ = theater_tx
                                .send(crate::messages::TheaterCommand::SendMessage {
                                    actor_id: actor_id.clone(),
                                    actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                        data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                    }),
                                })
                                .await;
                        },
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            // Partial chunk - read what's available
                            let mut partial_buffer = vec![0; chunk_size];
                            if let Ok(n) = reader.read(&mut partial_buffer).await {
                                if n > 0 {
                                    // Send the partial chunk to the actor
                                    let data = partial_buffer[0..n].to_vec();
                                    let _ = theater_tx
                                        .send(crate::messages::TheaterCommand::SendMessage {
                                            actor_id: actor_id.clone(),
                                            actor_message: crate::messages::ActorMessage::Send(crate::messages::ActorSend {
                                                data: serde_json::to_vec(&(if is_stdout { "handle-stdout" } else { "handle-stderr" }, process_id, data)).unwrap(),
                                            }),
                                        })
                                        .await;
                                }
                            }
                            break;
                        },
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            }
        }
    }
}
