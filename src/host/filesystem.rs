use crate::actor_handle::ActorHandle;
use crate::Store;
use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use tracing::{error, info};
use wasmtime::StoreContextMut;

pub struct FileSystemHost {
    path: PathBuf,
    actor_handle: ActorHandle,
}

impl FileSystemHost {
    pub fn new(path: PathBuf, actor_handle: ActorHandle) -> Self {
        Self { path, actor_handle }
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
            move |_ctx: StoreContextMut<'_, Store>, (file_path,): (String,)| -> Result<(String,)> {
                info!("Reading file {:?}", file_path);
                let file_path = Path::new(&file_path);

                // append the file path to the allowed path
                let file_path = allowed_path.join(file_path);

                info!("File path is allowed");

                let file = File::open(file_path).expect("could not open file");
                let mut reader = BufReader::new(file);
                let mut contents = String::new();
                reader
                    .read_to_string(&mut contents)
                    .expect("could not read file");
                info!("File read successfully");
                Ok((contents,))
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
