//! # Filesystem Handler
//!
//! Provides filesystem access capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to read, write, list, delete files and directories with
//! permission-based access control.
//!
//! ## Architecture
//!
//! This handler uses wasmtime's bindgen to generate type-safe Host traits from
//! the WASI Filesystem WIT definitions. This ensures compile-time verification that
//! our implementation matches the WASI specification.

pub mod events;
pub mod bindings;
pub mod host_impl;
pub mod types;
mod path_validation;
mod operations;
mod command_execution;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tracing::info;

use theater::actor::handle::ActorHandle;
use theater::config::actor_manifest::FileSystemHandlerConfig;
use theater::config::permissions::FileSystemPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

pub use events::FilesystemEventData;
pub use types::{FileSystemError, Descriptor, DescriptorType, DescriptorFlags, DirectoryEntryStream};
pub use host_impl::FilesystemPreopens;

/// Handler for providing filesystem access to WebAssembly actors
#[derive(Clone)]
pub struct FilesystemHandler {
    path: PathBuf,
    allowed_commands: Option<Vec<String>>,
    permissions: Option<FileSystemPermissions>,
}

impl FilesystemHandler {
    pub fn new(
        config: FileSystemHandlerConfig,
        permissions: Option<FileSystemPermissions>,
    ) -> Self {
        let path: PathBuf = match config.new_dir {
            Some(true) => Self::create_temp_dir().expect("Failed to create temp directory"),
            _ => PathBuf::from(config.path.clone().expect("Path must be provided")),
        };

        info!(
            "Creating filesystem handler with path: {:?}, permissions: {:?}",
            path, permissions
        );

        Self {
            path,
            allowed_commands: config.allowed_commands,
            permissions,
        }
    }

    fn create_temp_dir() -> anyhow::Result<PathBuf> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let random_num: u32 = rng.gen();

        let temp_base = PathBuf::from("/tmp/theater");
        std::fs::create_dir_all(&temp_base)?;

        let temp_dir = temp_base.join(random_num.to_string());
        std::fs::create_dir(&temp_dir)?;

        Ok(temp_dir)
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Handler for FilesystemHandler
{
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting filesystem handler on path {:?}", self.path);

        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("Filesystem handler received shutdown signal");
            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
        _ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        // Setup the Theater simple filesystem interface (for backwards compatibility)
        operations::setup_host_functions(self, actor_component)?;

        // Setup WASI filesystem interfaces using bindgen-generated add_to_linker
        info!("Setting up WASI filesystem interfaces using bindgen");

        // Set up preopened directories for this actor using ActorStore extensions
        let preopens = vec![(self.path.clone(), "/".to_string())];
        actor_component.actor_store.set_extension(FilesystemPreopens(preopens));
        info!("Set filesystem preopens in ActorStore extensions");

        use crate::bindings;
        use theater::actor::ActorStore;

        // Add wasi:filesystem/types interface
        bindings::wasi::filesystem::types::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        info!("wasi:filesystem/types interface added");

        // Add wasi:filesystem/preopens interface
        bindings::wasi::filesystem::preopens::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        info!("wasi:filesystem/preopens interface added");

        info!("WASI filesystem interfaces setup complete");
        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance,
    ) -> anyhow::Result<()> {
        info!("No export functions needed for filesystem handler");
        Ok(())
    }

    fn name(&self) -> &str {
        "filesystem"
    }

    fn imports(&self) -> Option<Vec<String>> {
        // Handler provides both Theater-specific and WASI filesystem interfaces
        Some(vec![
            "theater:simple/filesystem".to_string(),
            "wasi:filesystem/types@0.2.3".to_string(),
            "wasi:filesystem/preopens@0.2.3".to_string(),
        ])
    }

    fn exports(&self) -> Option<Vec<String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theater::config::actor_manifest::FileSystemHandlerConfig;

    #[test]
    fn test_handler_creation() {
        let config = FileSystemHandlerConfig {
            path: Some(std::path::PathBuf::from("/tmp")),
            new_dir: Some(false),
            allowed_commands: None,
        };

        let handler = FilesystemHandler::new(config, None);
        assert_eq!(handler.name(), "filesystem");
        // Note: imports() returns Vec<String> with all filesystem interfaces
        assert!(handler.imports().is_some());
        assert_eq!(handler.exports(), None);
    }

    #[test]
    fn test_handler_clone() {
        let config = FileSystemHandlerConfig {
            path: Some(std::path::PathBuf::from("/tmp")),
            new_dir: Some(false),
            allowed_commands: None,
        };

        let handler = FilesystemHandler::new(config, None);
        let cloned = handler.clone();
        assert_eq!(handler.path(), cloned.path());
    }

    #[test]
    fn test_temp_dir_creation() {
        let config = FileSystemHandlerConfig {
            path: None,
            new_dir: Some(true),
            allowed_commands: None,
        };

        let handler = FilesystemHandler::new(config, None);
        assert!(handler.path().exists());
        assert!(handler.path().starts_with("/tmp/theater"));
    }
}
