//! Filesystem operations implementation

mod basic_ops;
mod commands;

pub use basic_ops::*;
pub use commands::*;

use tracing::info;

use theater::events::EventPayload;
use theater::wasm::ActorComponent;

use crate::events::FilesystemEventData;
use crate::FilesystemHandler;

/// Setup all filesystem host functions
pub fn setup_host_functions<E>(
    handler: &FilesystemHandler,
    actor_component: &mut ActorComponent<E>,
) -> anyhow::Result<()>
where
    E: EventPayload + Clone + From<FilesystemEventData>,
{
    info!("Setting up filesystem host functions");

    // Record setup start
    actor_component.actor_store.record_handler_event(
        "filesystem-setup".to_string(),
        FilesystemEventData::HandlerSetupStart,
        Some("Starting filesystem host function setup".to_string()),
    );

    let mut interface = match actor_component.linker.instance("theater:simple/filesystem") {
        Ok(interface) => {
            actor_component.actor_store.record_handler_event(
                "filesystem-setup".to_string(),
                FilesystemEventData::LinkerInstanceSuccess,
                Some("Successfully created linker instance".to_string()),
            );
            interface
        }
        Err(e) => {
            actor_component.actor_store.record_handler_event(
                "filesystem-setup".to_string(),
                FilesystemEventData::HandlerSetupError {
                    error: e.to_string(),
                    step: "linker_instance".to_string(),
                },
                Some(format!("Failed to create linker instance: {}", e)),
            );
            return Err(anyhow::anyhow!(
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
    actor_component.actor_store.record_handler_event(
        "filesystem-setup".to_string(),
        FilesystemEventData::HandlerSetupSuccess,
        Some("Filesystem host functions setup completed successfully".to_string()),
    );

    Ok(())
}
