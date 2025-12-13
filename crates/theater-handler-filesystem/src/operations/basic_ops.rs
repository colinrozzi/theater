//! Basic filesystem operations (read, write, list, delete, create, path-exists)

use std::fs::File;
use std::io::{BufReader, Read, Write};
use tracing::{error, info};
use wasmtime::component::LinkerInstance;
use wasmtime::StoreContextMut;

use theater::actor::store::ActorStore;
use theater::events::filesystem::FilesystemEventData;
use theater::events::{ChainEventData, EventData};

use crate::path_validation::resolve_and_validate_path;
use crate::FilesystemHandler;

pub fn setup_read_file(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap(
            "read-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> anyhow::Result<(Result<Vec<u8>, String>,)> {
                let file_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
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
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap read-file function: {}", e))?;

    Ok(())
}

pub fn setup_write_file(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap(
            "write-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path, contents): (String, String)|
                  -> anyhow::Result<(Result<(), String>,)> {
                let file_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
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
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap write-file function: {}", e))?;

    Ok(())
}

pub fn setup_list_files(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap(
            "list-files",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> anyhow::Result<(Result<Vec<String>, String>,)> {
                let dir_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
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
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap list-files function: {}", e))?;

    Ok(())
}

pub fn setup_delete_file(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap(
            "delete-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> anyhow::Result<(Result<(), String>,)> {
                let file_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
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
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap delete-file function: {}", e))?;

    Ok(())
}

pub fn setup_create_dir(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap(
            "create-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> anyhow::Result<(Result<(), String>,)> {
                let dir_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
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
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap create-dir function: {}", e))?;

    Ok(())
}

pub fn setup_delete_dir(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap(
            "delete-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> anyhow::Result<(Result<(), String>,)> {
                let dir_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
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
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap delete-dir function: {}", e))?;

    Ok(())
}

pub fn setup_path_exists(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap(
            "path-exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_path,): (String,)|
                  -> anyhow::Result<(Result<bool, String>,)> {
                let path = match resolve_and_validate_path(
                    filesystem_handler.path(),
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
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap path-exists function: {}", e))?;

    Ok(())
}
