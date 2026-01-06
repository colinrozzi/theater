//! Filesystem operations implementation

mod basic_ops;
mod commands;

pub use basic_ops::*;
pub use commands::*;

use tracing::info;

use theater::wasm::ActorComponent;

use crate::FilesystemHandler;

/// Setup all filesystem host functions
pub fn setup_host_functions(
    handler: &FilesystemHandler,
    actor_component: &mut ActorComponent,
) -> anyhow::Result<()> {
    info!("Setting up filesystem host functions");

    // Record setup start
    let mut interface = match actor_component.linker.instance("theater:simple/filesystem") {
        Ok(interface) => {            interface
        }
        Err(e) => {            return Err(anyhow::anyhow!(
                "Could not instantiate theater:simple/filesystem: {}",
                e
            ));
        }
    };

    // Setup all the functions
    setup_read_file(handler, &mut interface)?;
    setup_write_file(handler, &mut interface)?;
    setup_list_files(handler, &mut interface)?;
    setup_delete_file(handler, &mut interface)?;
    setup_create_dir(handler, &mut interface)?;
    setup_delete_dir(handler, &mut interface)?;
    setup_path_exists(handler, &mut interface)?;
    setup_execute_command(handler, &mut interface)?;
    setup_execute_nix_command(handler, &mut interface)?;

    // Record overall setup completion
    Ok(())
}
