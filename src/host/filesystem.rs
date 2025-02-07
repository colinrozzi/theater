use crate::actor_handle::ActorHandle;
use crate::config::FileSystemHandlerConfig;
use crate::ActorStore;
use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use tracing::{error, info};
use wasmtime::StoreContextMut;

pub struct FileSystemHost {
    path: PathBuf,
    actor_handle: ActorHandle,
}

impl FileSystemHost {
    pub fn new(config: FileSystemHandlerConfig, actor_handle: ActorHandle) -> Self {
        Self {
            path: config.path,
            actor_handle,
        }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for filesystem");
        let mut actor = self.actor_handle.inner().lock().await;
        let mut interface = actor
            .linker
            .instance("ntwk:theater/filesystem")
            .expect("could not instantiate ntwk:theater/filesystem");

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "read-file",
            move |_ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<Vec<u8>, String>,)> {
                info!("Reading file {:?}", file_path);
                let file_path = Path::new(&file_path);

                // append the file path to the allowed path
                let file_path = allowed_path.join(file_path);

                info!("File path is allowed");

                let file = match File::open(file_path) {
                    Ok(f) => f,
                    Err(e) => return Ok((Err(e.to_string()),)),
                };

                let mut reader = BufReader::new(file);
                let mut contents = Vec::new();
                if let Err(e) = reader.read_to_end(&mut contents) {
                    return Ok((Err(e.to_string()),));
                }

                info!("File read successfully");
                Ok((Ok(contents),))
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "write-file",
            move |_ctx: StoreContextMut<'_, ActorStore>,
                  (file_path, contents): (String, String)|
                  -> Result<(Result<(), String>,)> {
                info!("Writing file {:?}", file_path);
                let file_path = Path::new(&file_path);

                // append the file path to the allowed path
                let file_path = allowed_path.join(file_path);

                info!("File path: {:?}", file_path);

                match File::create(file_path) {
                    Ok(mut file) => match file.write_all(contents.as_bytes()) {
                        Ok(_) => {
                            info!("File written successfully");
                            Ok((Ok(()),))
                        }
                        Err(e) => Ok((Err(e.to_string()),)),
                    },
                    Err(e) => Ok((Err(e.to_string()),)),
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "list-files",
            move |_ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<Vec<String>, String>,)> {
                info!("Listing files in {:?}", dir_path);
                let dir_path = Path::new(&dir_path);

                // append the file path to the allowed path
                let dir_path = allowed_path.join(dir_path);

                match dir_path.read_dir() {
                    Ok(entries) => {
                        let files: Result<Vec<String>, String> = Ok(entries
                            .filter_map(|entry| {
                                entry.ok().and_then(|e| e.file_name().into_string().ok())
                            })
                            .collect());
                        info!("Files listed successfully");
                        Ok((files,))
                    }
                    Err(e) => Ok((Err(e.to_string()),)),
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "delete-file",
            move |_ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                info!("Deleting file {:?}", file_path);
                let file_path = Path::new(&file_path);

                // append the file path to the allowed path
                let file_path = allowed_path.join(file_path);

                match std::fs::remove_file(file_path) {
                    Ok(_) => {
                        info!("File deleted successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e.to_string()),)),
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "create-dir",
            move |_ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                info!("Creating directory {:?}", dir_path);
                let dir_path = Path::new(&dir_path);

                // append the file path to the allowed path
                let dir_path = allowed_path.join(dir_path);

                info!("Creating directory at {:?}", dir_path);

                match std::fs::create_dir(dir_path) {
                    Ok(_) => {
                        info!("Directory created successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => {
                        info!("Failed to create directory");
                        error!("Error: {:?}", e);
                        Ok((Err(e.to_string()),))
                    }
                }
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "delete-dir",
            move |_ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                info!("Deleting directory {:?}", dir_path);
                let dir_path = Path::new(&dir_path);

                // append the file path to the allowed path
                let dir_path = allowed_path.join(dir_path);

                match std::fs::remove_dir_all(dir_path) {
                    Ok(_) => {
                        info!("Directory deleted successfully");
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e.to_string()),)),
                }
            },
        );

        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("No exports for filesystem");
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        info!("FILESYSTEM starting on path {:?}", self.path);
        Ok(())
    }
}
