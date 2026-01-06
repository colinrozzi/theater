//! Basic filesystem operations (read, write, list, delete, create, path-exists)

use std::fs::File;
use std::io::{BufReader, Read, Write};
use tracing::{error, info};
use wasmtime::component::LinkerInstance;
use wasmtime::StoreContextMut;

use theater::actor::store::ActorStore;


use crate::events::FilesystemEventData;
use crate::path_validation::resolve_and_validate_path;
use crate::FilesystemHandler;

pub fn setup_read_file(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()>
{
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
                                                return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                
                info!("Reading file {:?}", file_path);

                let file = match File::open(&file_path) {
                    Ok(f) => f,
                    Err(e) => {
                                                return Ok((Err(e.to_string()),));
                    }
                };

                let mut reader = BufReader::new(file);
                let mut contents = Vec::new();
                if let Err(e) = reader.read_to_end(&mut contents) {
                                        return Ok((Err(e.to_string()),));
                }

                
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
) -> anyhow::Result<()>
{
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
                                                return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                
                info!("Writing file {:?}", file_path);

                match File::create(&file_path) {
                    Ok(mut file) => match file.write_all(contents.as_bytes()) {
                        Ok(_) => {
                            
                            info!("File written successfully");
                            Ok((Ok(()),))
                        }
                        Err(e) => {
                            
                            Ok((Err(e.to_string()),))
                        }
                    },
                    Err(e) => {
                        
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
) -> anyhow::Result<()>
{
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
                                                return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                
                info!("Listing files in {:?}", dir_path);

                match dir_path.read_dir() {
                    Ok(entries) => {
                        let files: Vec<String> = entries
                            .filter_map(|entry| {
                                entry.ok().and_then(|e| e.file_name().into_string().ok())
                            })
                            .collect();

                        
                        info!("Files listed successfully");
                        Ok((Ok(files),))
                    }
                    Err(e) => {
                        
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
) -> anyhow::Result<()>
{
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
                                                return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                
                info!("Deleting file {:?}", file_path);

                match std::fs::remove_file(&file_path) {
                    Ok(_) => {
                        
                        info!("File deleted successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => {
                        
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
) -> anyhow::Result<()>
{
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
                                                return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                
                info!("Creating directory {:?}", dir_path);

                match std::fs::create_dir(&dir_path) {
                    Ok(_) => {
                        
                        info!("Directory created successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => {
                        
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
) -> anyhow::Result<()>
{
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
                                                return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                
                info!("Deleting directory {:?}", dir_path);

                match std::fs::remove_dir_all(&dir_path) {
                    Ok(_) => {
                        
                        info!("Directory deleted successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => {
                        
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
) -> anyhow::Result<()>
{
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
                                                return Ok((Err(format!("Permission denied: {}", e)),));
                    }
                };

                
                info!("Checking if path {:?} exists", path);

                let exists = path.exists();

                
                Ok((Ok(exists),))
            },
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap path-exists function: {}", e))?;

    Ok(())
}
