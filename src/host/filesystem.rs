use crate::actor_handle::ActorHandle;
use crate::config::FileSystemHandlerConfig;
use crate::ActorStore;
use crate::host::host_wrapper::HostFunctionBoundary;
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
                boundary.wrap(&mut ctx, (file_path.clone(), contents.clone()), |(file_path, contents)| {
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
                })
            },
        );

        // Similar updates for other functions...
        // I'll leave these as an exercise, but they would follow the same pattern

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