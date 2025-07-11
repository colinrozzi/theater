use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::config::actor_manifest::FileSystemHandlerConfig;
use crate::config::enforcement::PermissionChecker;
use crate::events::filesystem::{CommandError, CommandResult, CommandSuccess, FilesystemEventData};
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::ActorComponent;
use crate::wasm::ActorInstance;
use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::future::Future;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::process::Command as AsyncCommand;
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileSystemHost {
    path: PathBuf,
    allowed_commands: Option<Vec<String>>,
    permissions: Option<crate::config::permissions::FileSystemPermissions>,
}

impl FileSystemHost {
    pub fn new(config: FileSystemHandlerConfig, permissions: Option<crate::config::permissions::FileSystemPermissions>) -> Self {
        let path: PathBuf;
        match config.new_dir {
            Some(true) => {
                path = Self::create_temp_dir().unwrap();
            }
            _ => {
                path = PathBuf::from(config.path.unwrap());
            }
        }
        info!("Filesystem host path: {:?}", path);
        Self {
            path,
            allowed_commands: config.allowed_commands,
            permissions,
        }
    }

    pub fn create_temp_dir() -> Result<PathBuf> {
        let mut rng = rand::thread_rng();
        let random_num: u32 = rng.gen();

        let temp_base = PathBuf::from("/tmp/theater");
        std::fs::create_dir_all(&temp_base)?;

        let temp_dir = temp_base.join(random_num.to_string());
        std::fs::create_dir(&temp_dir)?;

        Ok(temp_dir)
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up filesystem host functions");

        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "filesystem-setup".to_string(),
            data: EventData::Filesystem(FilesystemEventData::HandlerSetupStart),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Starting filesystem host function setup".to_string()),
        });

        let mut interface = match actor_component
            .linker
            .instance("theater:simple/filesystem")
        {
            Ok(interface) => {
                // Record successful linker instance creation
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "filesystem-setup".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::LinkerInstanceSuccess),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "filesystem-setup".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!("Could not instantiate theater:simple/filesystem: {}", e));
            }
        };

        let allowed_path = self.path.clone();
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "read-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<Vec<u8>, String>,)> {
                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_filesystem_operation(
                    &permissions,
                    "read",
                    Some(&file_path),
                    None,
                ) {
                    error!("Filesystem read permission denied: {}", e);
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/permission-denied".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                            operation: "read".to_string(),
                            path: file_path.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for read operation on {}", file_path)),
                    });
                    return Ok((Err(format!("Permission denied: {}", e)),));
                }

                // Record file read call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/read-file".to_string(),
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
                            event_type: "theater:simple/filesystem/read-file".to_string(),
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
                        event_type: "theater:simple/filesystem/read-file".to_string(),
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
                    event_type: "theater:simple/filesystem/read-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileReadResult {
                        contents: contents.clone(),
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
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "write-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path, contents): (String, String)|
                  -> Result<(Result<(), String>,)> {
                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_filesystem_operation(
                    &permissions,
                    "write",
                    Some(&file_path),
                    None,
                ) {
                    error!("Filesystem write permission denied: {}", e);
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/permission-denied".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                            operation: "write".to_string(),
                            path: file_path.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for write operation on {}", file_path)),
                    });
                    return Ok((Err(format!("Permission denied: {}", e)),));
                }

                // Record file write call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/write-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileWriteCall {
                        path: file_path.clone(),
                        contents: contents.clone().into(),
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
                info!("Base path: {:?}", allowed_path);

                match File::create(&file_path) {
                    Ok(mut file) => match file.write_all(contents.as_bytes()) {
                        Ok(_) => {
                            // Record file write result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/filesystem/write-file".to_string(),
                                data: EventData::Filesystem(FilesystemEventData::FileWriteResult {
                                    path: file_path.to_string_lossy().to_string(),
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
                                event_type: "theater:simple/filesystem/write-file".to_string(),
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
                            event_type: "theater:simple/filesystem/write-file".to_string(),
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
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "list-files",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<Vec<String>, String>,)> {
                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_filesystem_operation(
                    &permissions,
                    "list",
                    Some(&dir_path),
                    None,
                ) {
                    error!("Filesystem list permission denied: {}", e);
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/permission-denied".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                            operation: "list".to_string(),
                            path: dir_path.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for list operation on {}", dir_path)),
                    });
                    return Ok((Err(format!("Permission denied: {}", e)),));
                }

                // Record directory listed call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/list-files".to_string(),
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
                            event_type: "theater:simple/filesystem/list-files".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::DirectoryListResult {
                                entries: files.clone(),
                                path: dir_path.to_string_lossy().to_string(),
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
                            event_type: "theater:simple/filesystem/list-files".to_string(),
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
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "delete-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_filesystem_operation(
                    &permissions,
                    "delete",
                    Some(&file_path),
                    None,
                ) {
                    error!("Filesystem delete permission denied: {}", e);
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/permission-denied".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                            operation: "delete".to_string(),
                            path: file_path.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for delete operation on {}", file_path)),
                    });
                    return Ok((Err(format!("Permission denied: {}", e)),));
                }

                // Record file delete call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/delete-file".to_string(),
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
                            event_type: "theater:simple/filesystem/delete-file".to_string(),
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
                            event_type: "theater:simple/filesystem/delete-file".to_string(),
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
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "create-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_filesystem_operation(
                    &permissions,
                    "write",
                    Some(&dir_path),
                    None,
                ) {
                    error!("Filesystem create directory permission denied: {}", e);
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/permission-denied".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                            operation: "create-dir".to_string(),
                            path: dir_path.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for create-dir operation on {}", dir_path)),
                    });
                    return Ok((Err(format!("Permission denied: {}", e)),));
                }

                // Record directory created call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/create-dir".to_string(),
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
                            event_type: "theater:simple/filesystem/create-dir".to_string(),
                            data: EventData::Filesystem(
                                FilesystemEventData::DirectoryCreatedResult {
                                    success: true,
                                    path: dir_path.to_string_lossy().to_string(),
                                },
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
                            event_type: "theater:simple/filesystem/create-dir".to_string(),
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
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "delete-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_filesystem_operation(
                    &permissions,
                    "delete",
                    Some(&dir_path),
                    None,
                ) {
                    error!("Filesystem delete directory permission denied: {}", e);
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/permission-denied".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                            operation: "delete-dir".to_string(),
                            path: dir_path.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for delete-dir operation on {}", dir_path)),
                    });
                    return Ok((Err(format!("Permission denied: {}", e)),));
                }

                // Record directory deleted call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/delete-dir".to_string(),
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
                            event_type: "theater:simple/filesystem/delete-dir".to_string(),
                            data: EventData::Filesystem(
                                FilesystemEventData::DirectoryDeletedResult {
                                    success: true,
                                    path: dir_path.to_string_lossy().to_string(),
                                },
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
                            event_type: "theater:simple/filesystem/delete-dir".to_string(),
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
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "path-exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (path,): (String,)|
                  -> Result<(Result<bool, String>,)> {
                // PERMISSION CHECK BEFORE OPERATION
                if let Err(e) = PermissionChecker::check_filesystem_operation(
                    &permissions,
                    "read",
                    Some(&path),
                    None,
                ) {
                    error!("Filesystem path-exists permission denied: {}", e);
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/permission-denied".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                            operation: "path-exists".to_string(),
                            path: path.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for path-exists operation on {}", path)),
                    });
                    return Ok((Err(format!("Permission denied: {}", e)),));
                }

                // Record path exists call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/path-exists".to_string(),
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
                    event_type: "theater:simple/filesystem/path-exists".to_string(),
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

        let allowed_path = self.path.clone();
        let allowed_commands = self.allowed_commands.clone();
        let _ =
            interface.func_wrap_async(
                "execute-command",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (dir, command, args): (String, String, Vec<String>)|
                      -> Box<
                    dyn Future<Output = Result<(Result<CommandResult, String>,)>> + Send,
                > {
                    // Validate command if whitelist is configured
                    if let Some(allowed) = &allowed_commands {
                        if !allowed.contains(&command) {
                            return Box::new(async move {
                                Ok((Err(format!("Command '{}' not in allowed list", command)),))
                            });
                        }
                    }

                    // Record command execution event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/execute-command".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::CommandExecuted {
                            directory: dir.clone(),
                            command: command.clone(),
                            args: args.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!(
                            "Executing command '{}' in directory '{}'",
                            command, dir
                        )),
                    });

                    let dir_path = allowed_path.join(Path::new(&dir));
                    let args_refs: Vec<String> = args.clone();
                    let allowed_path = allowed_path.clone();
                    let command_clone = command.clone();

                    Box::new(async move {
                        match execute_command(
                            allowed_path,
                            &dir_path,
                            &command_clone,
                            &args_refs.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                        )
                        .await
                        {
                            Ok(result) => {
                                // Record successful
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/filesystem/command-result"
                                        .to_string(),
                                    data: EventData::Filesystem(
                                        FilesystemEventData::CommandCompleted {
                                            result: result.clone(),
                                        },
                                    ),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some("Command completed".to_string()),
                                });
                                Ok((Ok(result),))
                            }
                            Err(e) => Ok((Err(e.to_string()),)),
                        }
                    })
                },
            )?;

        let allowed_path = self.path.clone();

        let _ =
            interface.func_wrap_async(
                "execute-nix-command",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (dir, command): (String, String)|
                      -> Box<
                    dyn Future<Output = Result<(Result<CommandResult, String>,)>> + Send,
                > {
                    // Record nix command execution event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/execute-nix-command".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::NixCommandExecuted {
                            directory: dir.clone(),
                            command: command.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!(
                            "Executing nix command '{}' in directory '{}'",
                            command, dir
                        )),
                    });

                    let dir_path = allowed_path.join(Path::new(&dir));
                    let allowed_path = allowed_path.clone();
                    let command_clone = command.clone();

                    Box::new(async move {
                        match execute_nix_command(allowed_path, &dir_path, &command_clone).await {
                            Ok(result) => {
                                // Record successful execution
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/filesystem/nix-command-result"
                                        .to_string(),
                                    data: EventData::Filesystem(
                                        FilesystemEventData::CommandCompleted {
                                            result: result.clone(),
                                        },
                                    ),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some("Nix command completed".to_string()),
                                });
                                Ok((Ok(result),))
                            }
                            Err(e) => Ok((Err(e.to_string()),)),
                        }
                    })
                },
            )?;

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "filesystem-setup".to_string(),
            data: EventData::Filesystem(FilesystemEventData::HandlerSetupSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Filesystem host functions setup completed successfully".to_string()),
        });

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("No functions needed for filesystem");
        Ok(())
    }

    pub async fn start(
        &self,
        _actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("FILESYSTEM starting on path {:?}", self.path);
        Ok(())
    }
}

async fn execute_command(
    allowed_path: PathBuf,
    dir: &Path,
    cmd: &str,
    args: &[&str],
) -> Result<CommandResult> {
    // Validate that the directory is within our allowed path
    if !dir.starts_with(&allowed_path) {
        return Ok(CommandResult::Error(CommandError {
            message: "Directory not within allowed path".to_string(),
        }));
    }

    if cmd != "nix" {
        return Ok(CommandResult::Error(CommandError {
            message: "Command not allowed".to_string(),
        }));
    }

    if args
        != &[
            "develop",
            "--command",
            "bash",
            "-c",
            "cargo component build --target wasm32-unknown-unknown --release",
        ]
        && args != &["flake", "init"]
    {
        info!("Args not allowed");
        info!("{:?}", args);
        return Ok(CommandResult::Error(CommandError {
            message: "Args not allowed".to_string(),
        }));
    }

    info!("Executing command: {} {:?}", cmd, args);

    // Execute the command
    let output = AsyncCommand::new(cmd)
        .current_dir(dir)
        .args(args)
        .output()
        .await?;

    info!("Command executed");
    info!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    info!("stderr: {}", String::from_utf8_lossy(&output.stderr));
    info!("exit code: {}", output.status.code().unwrap());

    Ok(CommandResult::Success(CommandSuccess {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    }))
}

async fn execute_nix_command(
    allowed_path: PathBuf,
    dir: &Path,
    command: &str,
) -> Result<CommandResult> {
    execute_command(allowed_path, dir, "nix", &["develop", "--command", command]).await
}
