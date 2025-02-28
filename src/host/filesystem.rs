use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::FileSystemHandlerConfig;
use crate::events::filesystem::FilesystemEventData;
use crate::events::{ChainEventData, EventData};
use crate::wasm::ActorComponent;
use crate::wasm::ActorInstance;
use crate::ActorStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{error, info};
use wasmtime::StoreContextMut;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum FileSystemCommand {
    ReadFile { path: String },
    WriteFile { path: String, contents: String },
    ListFiles { path: String },
    DeleteFile { path: String },
    CreateDir { path: String },
    DeleteDir { path: String },
    PathExists { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum FileSystemResponse {
    ReadFile(Result<Vec<u8>, String>),
    WriteFile(Result<(), String>),
    ListFiles(Result<Vec<String>, String>),
    DeleteFile(Result<(), String>),
    CreateDir(Result<(), String>),
    DeleteDir(Result<(), String>),
    PathExists(Result<bool, String>),
}

#[derive(Error, Debug)]
pub enum FileSystemError {
    #[error("Path error: {0}")]
    PathError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

pub struct FileSystemHost {
    path: PathBuf,
}

impl FileSystemHost {
    pub fn new(config: FileSystemHandlerConfig) -> Self {
        Self { path: config.path }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up filesystem host functions");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/filesystem")
            .expect("could not instantiate ntwk:theater/filesystem");

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "read-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<Vec<u8>, String>,)> {
                // Record file read call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/read-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileReadCall {
                        path: file_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Read file {:?}", file_path)),
                });

                let file_path = allowed_path.join(Path::new(&file_path));
                info!("Reading file {:?}", file_path);

                let file = match File::open(&file_path) {
                    Ok(f) => f,
                    Err(e) => {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/read-file".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::Error {
                                operation: "open".to_string(),
                                path: file_path.to_string_lossy().to_string(),
                                message: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Error opening file {:?}", file_path)),
                        });
                        return Ok((Err(e.to_string()),));
                    }
                };

                let mut reader = BufReader::new(file);
                let mut contents = Vec::new();
                if let Err(e) = reader.read_to_end(&mut contents) {
                    // Record error event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/filesystem/read-file".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::Error {
                            operation: "read".to_string(),
                            path: file_path.to_string_lossy().to_string(),
                            message: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Error reading file {:?}", file_path)),
                    });
                    return Ok((Err(e.to_string()),));
                }

                // Record file read result event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/read-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileReadResult {
                        bytes_read: contents.len(),
                        success: true,
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Successfully read {} bytes from file {:?}",
                        contents.len(),
                        file_path
                    )),
                });

                info!("File read successfully");
                Ok((Ok(contents),))
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "write-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path, contents): (String, String)|
                  -> Result<(Result<(), String>,)> {
                // Record file write call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/write-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileWriteCall {
                        path: file_path.clone(),
                        data_size: contents.len(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Writing {} bytes to file {:?}",
                        contents.len(),
                        file_path
                    )),
                });

                let file_path = allowed_path.join(Path::new(&file_path));
                info!("Writing file {:?}", file_path);

                match File::create(&file_path) {
                    Ok(mut file) => match file.write_all(contents.as_bytes()) {
                        Ok(_) => {
                            // Record file write result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/filesystem/write-file".to_string(),
                                data: EventData::Filesystem(FilesystemEventData::FileWriteResult {
                                    path: file_path.to_string_lossy().to_string(),
                                    bytes_written: contents.len(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Successfully wrote {} bytes to file {:?}",
                                    contents.len(),
                                    file_path
                                )),
                            });

                            info!("File written successfully");
                            Ok((Ok(()),))
                        }
                        Err(e) => {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/filesystem/write-file".to_string(),
                                data: EventData::Filesystem(FilesystemEventData::Error {
                                    operation: "write".to_string(),
                                    path: file_path.to_string_lossy().to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error writing to file {:?}: {}",
                                    file_path, e
                                )),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    },
                    Err(e) => {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/write-file".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::Error {
                                operation: "create".to_string(),
                                path: file_path.to_string_lossy().to_string(),
                                message: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Error creating file {:?}: {}",
                                file_path, e
                            )),
                        });

                        Ok((Err(e.to_string()),))
                    }
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "list-files",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<Vec<String>, String>,)> {
                // Record directory listed call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/list-files".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::DirectoryListedCall {
                        path: dir_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Listing files in directory {:?}", dir_path)),
                });

                info!("Listing files in {:?}", dir_path);
                let dir_path = Path::new(&dir_path);

                // append the file path to the allowed path
                let dir_path = allowed_path.join(dir_path);

                match dir_path.read_dir() {
                    Ok(entries) => {
                        let files: Vec<String> = entries
                            .filter_map(|entry| {
                                entry.ok().and_then(|e| e.file_name().into_string().ok())
                            })
                            .collect();

                        // Record directory list result event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/list-files".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::DirectoryListResult {
                                entries: files.clone(),
                                success: true,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Successfully listed {} files in directory {:?}",
                                files.len(),
                                dir_path
                            )),
                        });

                        info!("Files listed successfully");
                        Ok((Ok(files),))
                    }
                    Err(e) => {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/list-files".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::Error {
                                operation: "list".to_string(),
                                path: dir_path.to_string_lossy().to_string(),
                                message: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Error listing files in directory {:?}: {}",
                                dir_path, e
                            )),
                        });

                        Ok((Err(e.to_string()),))
                    }
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "delete-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // Record file delete call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/delete-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileDeleteCall {
                        path: file_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Deleting file {:?}", file_path)),
                });

                let file_path = allowed_path.join(Path::new(&file_path));
                info!("Deleting file {:?}", file_path);

                match std::fs::remove_file(&file_path) {
                    Ok(_) => {
                        // Record file delete result event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/delete-file".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::FileDeleteResult {
                                path: file_path.to_string_lossy().to_string(),
                                success: true,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Successfully deleted file {:?}", file_path)),
                        });

                        info!("File deleted successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/delete-file".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::Error {
                                operation: "delete".to_string(),
                                path: file_path.to_string_lossy().to_string(),
                                message: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Error deleting file {:?}: {}",
                                file_path, e
                            )),
                        });

                        Ok((Err(e.to_string()),))
                    }
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "create-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // Record directory created call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/create-dir".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::DirectoryCreatedCall {
                        path: dir_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Creating directory {:?}", dir_path)),
                });

                info!("Allowed path: {:?}", allowed_path);
                let dir_path = allowed_path.join(Path::new(&dir_path));
                info!("Creating directory {:?}", dir_path);

                match std::fs::create_dir(&dir_path) {
                    Ok(_) => {
                        // Record directory created result event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/create-dir".to_string(),
                            data: EventData::Filesystem(
                                FilesystemEventData::DirectoryCreatedResult { success: true },
                            ),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Successfully created directory {:?}",
                                dir_path
                            )),
                        });

                        info!("Directory created successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/create-dir".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::Error {
                                operation: "create_dir".to_string(),
                                path: dir_path.to_string_lossy().to_string(),
                                message: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Error creating directory {:?}: {}",
                                dir_path, e
                            )),
                        });

                        Ok((Err(e.to_string()),))
                    }
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "delete-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // Record directory deleted call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/delete-dir".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::DirectoryDeletedCall {
                        path: dir_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Deleting directory {:?}", dir_path)),
                });

                let dir_path = allowed_path.join(Path::new(&dir_path));
                info!("Deleting directory {:?}", dir_path);

                match std::fs::remove_dir_all(&dir_path) {
                    Ok(_) => {
                        // Record directory deleted result event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/delete-dir".to_string(),
                            data: EventData::Filesystem(
                                FilesystemEventData::DirectoryDeletedResult { success: true },
                            ),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Successfully deleted directory {:?}",
                                dir_path
                            )),
                        });

                        info!("Directory deleted successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/filesystem/delete-dir".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::Error {
                                operation: "delete_dir".to_string(),
                                path: dir_path.to_string_lossy().to_string(),
                                message: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Error deleting directory {:?}: {}",
                                dir_path, e
                            )),
                        });

                        Ok((Err(e.to_string()),))
                    }
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "path-exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (path,): (String,)|
                  -> Result<(Result<bool, String>,)> {
                // Record path exists call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/path-exists".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::PathExistsCall {
                        path: path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Checking if path {:?} exists", path)),
                });

                let path = allowed_path.join(Path::new(&path));
                info!("Checking if path {:?} exists", path);

                let exists = path.exists();

                // Record path exists result event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/filesystem/path-exists".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::PathExistsResult {
                        path: path.to_string_lossy().to_string(),
                        exists,
                        success: true,
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Path {:?} exists: {}", path, exists)),
                });

                Ok((Ok(exists),))
            },
        );

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("No functions needed for filesystem");
        Ok(())
    }

    pub async fn start(&self, _actor_handle: ActorHandle) -> Result<()> {
        info!("FILESYSTEM starting on path {:?}", self.path);
        Ok(())
    }
}
