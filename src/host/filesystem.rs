use crate::actor_handle::ActorHandle;
use crate::config::FileSystemHandlerConfig;
use crate::Store;
use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use tracing::info;
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

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "write-file",
            move |_ctx: StoreContextMut<'_, Store>, (file_path, contents): (String, String)| {
                info!("Writing file {:?}", file_path);
                let file_path = Path::new(&file_path);

                // append the file path to the allowed path
                let file_path = allowed_path.join(file_path);

                let mut file = File::create(file_path).expect("could not create file");
                file.write_all(contents.as_bytes())
                    .expect("could not write file");
                info!("File written successfully");
                Ok(())
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "list-files",
            move |_ctx: StoreContextMut<'_, Store>,
                  (dir_path,): (String,)|
                  -> Result<(Vec<String>,)> {
                info!("Listing files in {:?}", dir_path);
                let dir_path = Path::new(&dir_path);

                // append the file path to the allowed path
                let dir_path = allowed_path.join(dir_path);

                let entries = dir_path
                    .read_dir()
                    .expect("could not read directory")
                    .map(|entry| {
                        entry
                            .expect("could not read entry")
                            .file_name()
                            .into_string()
                            .expect("could not convert OsString to String")
                    })
                    .collect();
                info!("Files listed successfully");
                Ok((entries,))
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "delete-file",
            move |_ctx: StoreContextMut<'_, Store>, (file_path,): (String,)| {
                info!("Deleting file {:?}", file_path);
                let file_path = Path::new(&file_path);

                // append the file path to the allowed path
                let file_path = allowed_path.join(file_path);

                std::fs::remove_file(file_path).expect("could not delete file");
                info!("File deleted successfully");
                Ok(())
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "create-dir",
            move |_ctx: StoreContextMut<'_, Store>, (dir_path,): (String,)| {
                info!("Creating directory {:?}", dir_path);
                let dir_path = Path::new(&dir_path);

                // append the file path to the allowed path
                let dir_path = allowed_path.join(dir_path);

                std::fs::create_dir(dir_path).expect("could not create directory");
                info!("Directory created successfully");
                Ok(())
            },
        );

        let allowed_path = self.path.clone();

        let _ = interface.func_wrap(
            "delete-dir",
            move |_ctx: StoreContextMut<'_, Store>, (dir_path,): (String,)| {
                info!("Deleting directory {:?}", dir_path);
                let dir_path = Path::new(&dir_path);

                // append the file path to the allowed path
                let dir_path = allowed_path.join(dir_path);

                std::fs::remove_dir_all(dir_path).expect("could not delete directory");
                info!("Directory deleted successfully");
                Ok(())
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
