use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::config::actor_manifest::FileSystemHandlerConfig;

use crate::events::filesystem::{CommandError, CommandResult, CommandSuccess, FilesystemEventData};
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::ActorComponent;
use crate::wasm::ActorInstance;
use anyhow::Result;
use dunce;
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
    /// Resolve and validate a path against permissions
    ///
    /// This function:
    /// 1. For creation operations (write, create-dir): validates the parent directory exists and is allowed
    /// 2. For other operations (read, delete, list, etc.): validates the target path exists and is allowed
    /// 3. Resolves the path (handles ., .., etc.)
    /// 4. Checks if the resolved path is within allowed paths
    ///
    /// Returns the resolved path that should be used for the operation
    fn resolve_and_validate_path(
        &self,
        requested_path: &str,
        operation: &str, // <- Removed underscore
        permissions: &Option<crate::config::permissions::FileSystemPermissions>,
    ) -> Result<PathBuf, String> {
        // 1. Append requested path to base path
        let full_path = self.path.join(requested_path);

        // 2. Determine if this is a creation operation
        let is_creation = matches!(operation, "write" | "create-dir");

        // 3. For creation operations, validate the parent directory
        //    For other operations, validate the target path
        let path_to_validate = if is_creation {
            // For creation, we need to validate the parent directory
            full_path.parent().ok_or_else(|| {
                "Cannot determine parent directory for creation operation".to_string()
            })?
        } else {
            // For read/delete operations, validate the target path
            &full_path
        };

        // 4. Use dunce for robust path canonicalization
        let resolved_validation_path = dunce::canonicalize(path_to_validate).map_err(|e| {
            if is_creation {
                format!(
                    "Failed to resolve parent directory '{}' for creation operation: {}",
                    path_to_validate.display(),
                    e
                )
            } else {
                format!(
                    "Failed to resolve path '{}': {}",
                    path_to_validate.display(),
                    e
                )
            }
        })?;

        // 5. Check if resolved path is within allowed paths
        if let Some(perms) = permissions {
            if let Some(allowed_paths) = &perms.allowed_paths {
                let is_allowed = allowed_paths.iter().any(|allowed_path| {
                    // Canonicalize the allowed path for comparison using dunce
                    let allowed_canonical = dunce::canonicalize(allowed_path)
                        .unwrap_or_else(|_| PathBuf::from(allowed_path));

                    // Check if resolved path is within the allowed directory
                    resolved_validation_path == allowed_canonical
                        || resolved_validation_path.starts_with(&allowed_canonical)
                });

                if !is_allowed {
                    return Err(if is_creation {
                        format!(
                            "Parent directory '{}' not in allowed paths for creation operation: {:?}", 
                            resolved_validation_path.display(), 
                            allowed_paths
                        )
                    } else {
                        format!(
                            "Path '{}' not in allowed paths: {:?}",
                            resolved_validation_path.display(),
                            allowed_paths
                        )
                    });
                }
            }
        }

        // 6. For creation operations, construct the final path from canonicalized parent + filename
        //    For other operations, return the canonicalized path
        if is_creation {
            // For creation, we've validated the parent, now construct the target path
            // by appending the filename/dirname to the canonicalized parent directory
            let final_component = full_path.file_name().ok_or_else(|| {
                format!(
                    "Cannot determine target name for {} operation on path '{}'",
                    operation, requested_path
                )
            })?;

            Ok(resolved_validation_path.join(final_component))
        } else {
            // For read/delete, return the canonicalized path
            Ok(dunce::canonicalize(&full_path).map_err(|e| {
                format!(
                    "Failed to resolve target path '{}': {}",
                    full_path.display(),
                    e
                )
            })?)
        }
    }

    pub fn new(
        config: FileSystemHandlerConfig,
        permissions: Option<crate::config::permissions::FileSystemPermissions>,
    ) -> Self {
        let path: PathBuf;
        match config.new_dir {
            Some(true) => {
                path = Self::create_temp_dir().unwrap();
            }
            _ => {
                path = PathBuf::from(config.clone().path.unwrap());
            }
        }
        info!(
            "Creating filesystem host with config: {:?}, permissions: {:?}",
            config, permissions
        );
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

        let mut interface = match actor_component.linker.instance("theater:simple/filesystem") {
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
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/filesystem: {}",
                    e
                ));
            }
        };

        let _allowed_path = self.path.clone();
        let permissions = self.permissions.clone();

        let filesystem_host = self.clone();
        let _ = interface.func_wrap(
            "read-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> Result<(Result<Vec<u8>, String>,)> {
                // RESOLVE AND VALIDATE PATH
                let file_path = match filesystem_host.resolve_and_validate_path(
                    &requested_path,
                    "read",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Filesystem read permission denied: {}", e);
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/filesystem/permission-denied".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                                operation: "read".to_string(),
                                path: requested_path.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Permission denied for read operation on {}",
                                requested_path
                            )),
                        });
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                // Record file read call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/read-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileReadCall {
                        path: requested_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Read file {:?}", requested_path)),
                });

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

        let permissions = self.permissions.clone();

        let filesystem_host = self.clone();
        let _ = interface.func_wrap(
            "write-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path, contents): (String, String)|
                  -> Result<(Result<(), String>,)> {
                // RESOLVE AND VALIDATE PATH
                let file_path = match filesystem_host.resolve_and_validate_path(
                    &requested_path,
                    "write",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Filesystem write permission denied: {}", e);
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/filesystem/permission-denied".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                                operation: "write".to_string(),
                                path: requested_path.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Permission denied for write operation on {}",
                                requested_path
                            )),
                        });
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                // Record file write call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/write-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileWriteCall {
                        path: requested_path.clone(),
                        contents: contents.clone().into(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Writing {} bytes to file {:?}",
                        contents.len(),
                        requested_path
                    )),
                });

                info!("Writing file {:?}", file_path);

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

        let permissions = self.permissions.clone();

        let filesystem_host = self.clone();
        let _ = interface.func_wrap(
            "list-files",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> Result<(Result<Vec<String>, String>,)> {
                // RESOLVE AND VALIDATE PATH
                let dir_path = match filesystem_host.resolve_and_validate_path(
                    &requested_path,
                    "read",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Filesystem list permission denied: {}", e);
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/filesystem/permission-denied".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                                operation: "list".to_string(),
                                path: requested_path.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Permission denied for list operation on {}",
                                requested_path
                            )),
                        });
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                // Record directory listed call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/list-files".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::DirectoryListedCall {
                        path: requested_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Listing files in directory {:?}", requested_path)),
                });

                info!("Listing files in {:?}", dir_path);

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

        let filesystem_host = self.clone();
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "delete-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // RESOLVE AND VALIDATE PATH
                let file_path = match filesystem_host.resolve_and_validate_path(
                    &requested_path,
                    "delete",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Filesystem delete permission denied: {}", e);
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/filesystem/permission-denied".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                                operation: "delete".to_string(),
                                path: requested_path.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Permission denied for delete operation on {}",
                                requested_path
                            )),
                        });
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                // Record file delete call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/delete-file".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::FileDeleteCall {
                        path: requested_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Deleting file {:?}", requested_path)),
                });

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

        let filesystem_host = self.clone();
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "create-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // RESOLVE AND VALIDATE PATH
                let dir_path = match filesystem_host.resolve_and_validate_path(
                    &requested_path,
                    "write",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Filesystem create directory permission denied: {}", e);
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/filesystem/permission-denied".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                                operation: "create-dir".to_string(),
                                path: requested_path.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Permission denied for create-dir operation on {}",
                                requested_path
                            )),
                        });
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                // Record directory created call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/create-dir".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::DirectoryCreatedCall {
                        path: requested_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Creating directory {:?}", requested_path)),
                });

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

        let filesystem_host = self.clone();
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "delete-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                // RESOLVE AND VALIDATE PATH
                let dir_path = match filesystem_host.resolve_and_validate_path(
                    &requested_path,
                    "delete",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Filesystem delete directory permission denied: {}", e);
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/filesystem/permission-denied".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                                operation: "delete-dir".to_string(),
                                path: requested_path.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Permission denied for delete-dir operation on {}",
                                requested_path
                            )),
                        });
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                // Record directory deleted call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/delete-dir".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::DirectoryDeletedCall {
                        path: requested_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Deleting directory {:?}", requested_path)),
                });

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

        let filesystem_host = self.clone();
        let permissions = self.permissions.clone();

        let _ = interface.func_wrap(
            "path-exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> Result<(Result<bool, String>,)> {
                // RESOLVE AND VALIDATE PATH
                let path = match filesystem_host.resolve_and_validate_path(
                    &requested_path,
                    "read",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        error!("Filesystem path-exists permission denied: {}", e);
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/filesystem/permission-denied".to_string(),
                            data: EventData::Filesystem(FilesystemEventData::PermissionDenied {
                                operation: "path-exists".to_string(),
                                path: requested_path.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Permission denied for path-exists operation on {}",
                                requested_path
                            )),
                        });
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                // Record path exists call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/path-exists".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::PathExistsCall {
                        path: requested_path.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Checking if path {:?} exists", requested_path)),
                });

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

        let filesystem_host = self.clone();
        let permissions = self.permissions.clone();
        let allowed_commands = self.allowed_commands.clone();
        let _ =
            interface.func_wrap_async(
                "execute-command",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (requested_dir, command, args): (String, String, Vec<String>)|
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

                    // RESOLVE AND VALIDATE PATH
                    let dir_path = match filesystem_host.resolve_and_validate_path(
                        &requested_dir,
                        "execute",
                        &permissions,
                    ) {
                        Ok(path) => path,
                        Err(e) => {
                            return Box::new(async move {
                                Ok((Err(format!("Permission denied: {}", e)),))
                            });
                        }
                    };

                    // Record command execution event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/execute-command".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::CommandExecuted {
                            directory: requested_dir.clone(),
                            command: command.clone(),
                            args: args.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!(
                            "Executing command '{}' in directory '{}'",
                            command, requested_dir
                        )),
                    });

                    let args_refs: Vec<String> = args.clone();
                    let base_path = filesystem_host.path.clone();
                    let command_clone = command.clone();

                    Box::new(async move {
                        match execute_command(
                            base_path,
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

        let filesystem_host = self.clone();
        let permissions = self.permissions.clone();

        let _ =
            interface.func_wrap_async(
                "execute-nix-command",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (requested_dir, command): (String, String)|
                      -> Box<
                    dyn Future<Output = Result<(Result<CommandResult, String>,)>> + Send,
                > {
                    // RESOLVE AND VALIDATE PATH
                    let dir_path = match filesystem_host.resolve_and_validate_path(
                        &requested_dir,
                        "execute",
                        &permissions,
                    ) {
                        Ok(path) => path,
                        Err(e) => {
                            return Box::new(async move {
                                Ok((Err(format!("Permission denied: {}", e)),))
                            });
                        }
                    };

                    // Record nix command execution event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/filesystem/execute-nix-command".to_string(),
                        data: EventData::Filesystem(FilesystemEventData::NixCommandExecuted {
                            directory: requested_dir.clone(),
                            command: command.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!(
                            "Executing nix command '{}' in directory '{}'",
                            command, requested_dir
                        )),
                    });

                    let base_path = filesystem_host.path.clone();
                    let command_clone = command.clone();

                    Box::new(async move {
                        match execute_nix_command(base_path, &dir_path, &command_clone).await {
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
