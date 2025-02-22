use crate::actor_handle::ActorHandle;
use crate::actor_executor::ActorError;
use crate::config::FileSystemHandlerConfig;
use crate::wasm::Event;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{info, error};

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
        info!("Setting up filesystem host functions");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("No exports needed for filesystem");
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        info!("FILESYSTEM starting on path {:?}", self.path);
        Ok(())
    }

    async fn handle_command(&self, cmd: FileSystemCommand) -> Result<FileSystemResponse, FileSystemError> {
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

    async fn process_filesystem_event(&self, command: FileSystemCommand) -> Result<(), FileSystemError> {
        // Handle the command
        let response = self.handle_command(command).await?;
        
        // Create event with response
        let event = Event {
            event_type: "filesystem-response".to_string(),
            parent: None,
            data: serde_json::to_vec(&response)?,
        };

        // Send event to actor
        self.actor_handle.handle_event(event).await?;

        Ok(())
    }
}
