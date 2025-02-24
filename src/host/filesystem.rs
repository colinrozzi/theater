use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::FileSystemHandlerConfig;
use crate::host::host_wrapper::HostFunctionBoundary;
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
        let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "read-file");

        let _ = interface.func_wrap(
            "read-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<Vec<u8>, String>,)> {
                boundary.wrap(&mut ctx, (file_path.clone(),), |(file_path,)| {
                    let file_path = allowed_path.join(Path::new(&file_path));
                    info!("Reading file {:?}", file_path);

                    let file = match File::open(&file_path) {
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
                })
            },
        );

        let allowed_path = self.path.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "write-file");

        let _ = interface.func_wrap(
            "write-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path, contents): (String, String)|
                  -> Result<(Result<(), String>,)> {
                boundary.wrap(
                    &mut ctx,
                    (file_path.clone(), contents.clone()),
                    |(file_path, contents)| {
                        let file_path = allowed_path.join(Path::new(&file_path));
                        info!("Writing file {:?}", file_path);

                        match File::create(&file_path) {
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
                )
            },
        );

        let allowed_path = self.path.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "list-files");

        let _ = interface.func_wrap(
            "list-files",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<Vec<String>, String>,)> {
                boundary.wrap(&mut ctx, (), |_| {
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
                })
            },
        );

        let allowed_path = self.path.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "delete-file");

        let _ = interface.func_wrap(
            "delete-file",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (file_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                boundary.wrap(&mut ctx, file_path.clone(), |file_path| {
                    let file_path = allowed_path.join(Path::new(&file_path));
                    info!("Deleting file {:?}", file_path);

                    match std::fs::remove_file(&file_path) {
                        Ok(_) => {
                            info!("File deleted successfully");
                            Ok((Ok(()),))
                        }
                        Err(e) => Ok((Err(e.to_string()),)),
                    }
                })
            },
        );

        let allowed_path = self.path.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "create-dir");

        let _ = interface.func_wrap(
            "create-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                boundary.wrap(&mut ctx, dir_path.clone(), |dir_path| {
                    info!("Allowed path: {:?}", allowed_path);
                    let dir_path = allowed_path.join(Path::new(&dir_path));
                    info!("Creating directory {:?}", dir_path);

                    match std::fs::create_dir(&dir_path) {
                        Ok(_) => {
                            info!("Directory created successfully");
                            Ok((Ok(()),))
                        }
                        Err(e) => Ok((Err(e.to_string()),)),
                    }
                })
            },
        );

        let allowed_path = self.path.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "delete-dir");

        let _ = interface.func_wrap(
            "delete-dir",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (dir_path,): (String,)|
                  -> Result<(Result<(), String>,)> {
                boundary.wrap(&mut ctx, dir_path.clone(), |dir_path| {
                    let dir_path = allowed_path.join(Path::new(&dir_path));
                    info!("Deleting directory {:?}", dir_path);

                    match std::fs::remove_dir_all(&dir_path) {
                        Ok(_) => {
                            info!("Directory deleted successfully");
                            Ok((Ok(()),))
                        }
                        Err(e) => Ok((Err(e.to_string()),)),
                    }
                })
            },
        );

        /*
                let allowed_path = self.path.clone();
                let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "rename-file");

                let _ = interface.func_wrap(
                    "rename-file",
                    move |mut ctx: StoreContextMut<'_, ActorStore>,
                          (old_path, new_path): (String, String)|
                          -> Result<(Result<(), String>,)> {
                        boundary.wrap(
                            &mut ctx,
                            (old_path.clone(), new_path.clone()),
                            |(old_path, new_path)| {
                                let old_path = allowed_path.join(Path::new(&old_path));
                                let new_path = allowed_path.join(Path::new(&new_path));
                                info!("Renaming file {:?} to {:?}", old_path, new_path);

                                match std::fs::rename(&old_path, &new_path) {
                                    Ok(_) => {
                                        info!("File renamed successfully");
                                        Ok((Ok(()),))
                                    }
                                    Err(e) => Ok((Err(e.to_string()),)),
                                }
                            },
                        )
                    },
                );

                let allowed_path = self.path.clone();
                let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "rename-dir");

                let _ = interface.func_wrap(
                    "rename-dir",
                    move |mut ctx: StoreContextMut<'_, ActorStore>,
                          (old_path, new_path): (String, String)|
                          -> Result<(Result<(), String>,)> {
                        boundary.wrap(
                            &mut ctx,
                            (old_path.clone(), new_path.clone()),
                            |(old_path, new_path)| {
                                let old_path = allowed_path.join(Path::new(&old_path));
                                let new_path = allowed_path.join(Path::new(&new_path));
                                info!("Renaming directory {:?} to {:?}", old_path, new_path);

                                match std::fs::rename(&old_path, &new_path) {
                                    Ok(_) => {
                                        info!("Directory renamed successfully");
                                        Ok((Ok(()),))
                                    }
                                    Err(e) => Ok((Err(e.to_string()),)),
                                }
                            },
                        )
                    },
                );
        */

        let allowed_path = self.path.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/filesystem", "path-exists");

        let _ = interface.func_wrap(
            "path-exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (path,): (String,)|
                  -> Result<(Result<bool, String>,)> {
                boundary.wrap(&mut ctx, path.clone(), |path| {
                    let path = allowed_path.join(Path::new(&path));
                    info!("Checking if path {:?} exists", path);

                    Ok((Ok(path.exists()),))
                })
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

    async fn handle_command(
        &self,
        cmd: FileSystemCommand,
    ) -> Result<FileSystemResponse, FileSystemError> {
        match cmd {
            FileSystemCommand::ReadFile { path } => {
                let file_path = self.path.join(Path::new(&path));
                info!("Reading file {:?}", file_path);

                match File::open(&file_path) {
                    Ok(file) => {
                        let mut reader = BufReader::new(file);
                        let mut contents = Vec::new();
                        match reader.read_to_end(&mut contents) {
                            Ok(_) => Ok(FileSystemResponse::ReadFile(Ok(contents))),
                            Err(e) => Ok(FileSystemResponse::ReadFile(Err(e.to_string()))),
                        }
                    }
                    Err(e) => Ok(FileSystemResponse::ReadFile(Err(e.to_string()))),
                }
            }

            FileSystemCommand::WriteFile { path, contents } => {
                let file_path = self.path.join(Path::new(&path));
                info!("Writing file {:?}", file_path);

                match File::create(&file_path) {
                    Ok(mut file) => match file.write_all(contents.as_bytes()) {
                        Ok(_) => Ok(FileSystemResponse::WriteFile(Ok(()))),
                        Err(e) => Ok(FileSystemResponse::WriteFile(Err(e.to_string()))),
                    },
                    Err(e) => Ok(FileSystemResponse::WriteFile(Err(e.to_string()))),
                }
            }

            FileSystemCommand::ListFiles { path } => {
                let dir_path = self.path.join(Path::new(&path));
                info!("Listing files in {:?}", dir_path);

                match dir_path.read_dir() {
                    Ok(entries) => {
                        let files: Vec<String> = entries
                            .filter_map(|entry| {
                                entry.ok().and_then(|e| e.file_name().into_string().ok())
                            })
                            .collect();
                        Ok(FileSystemResponse::ListFiles(Ok(files)))
                    }
                    Err(e) => Ok(FileSystemResponse::ListFiles(Err(e.to_string()))),
                }
            }

            FileSystemCommand::DeleteFile { path } => {
                let file_path = self.path.join(Path::new(&path));
                info!("Deleting file {:?}", file_path);

                match std::fs::remove_file(&file_path) {
                    Ok(_) => Ok(FileSystemResponse::DeleteFile(Ok(()))),
                    Err(e) => Ok(FileSystemResponse::DeleteFile(Err(e.to_string()))),
                }
            }

            FileSystemCommand::CreateDir { path } => {
                let dir_path = self.path.join(Path::new(&path));
                info!("Creating directory {:?}", dir_path);

                match std::fs::create_dir(&dir_path) {
                    Ok(_) => Ok(FileSystemResponse::CreateDir(Ok(()))),
                    Err(e) => Ok(FileSystemResponse::CreateDir(Err(e.to_string()))),
                }
            }

            FileSystemCommand::DeleteDir { path } => {
                let dir_path = self.path.join(Path::new(&path));
                info!("Deleting directory {:?}", dir_path);

                match std::fs::remove_dir_all(&dir_path) {
                    Ok(_) => Ok(FileSystemResponse::DeleteDir(Ok(()))),
                    Err(e) => Ok(FileSystemResponse::DeleteDir(Err(e.to_string()))),
                }
            }

            FileSystemCommand::PathExists { path } => {
                let path = self.path.join(Path::new(&path));
                info!("Checking if path {:?} exists", path);

                Ok(FileSystemResponse::PathExists(Ok(path.exists())))
            }
        }
    }
}
