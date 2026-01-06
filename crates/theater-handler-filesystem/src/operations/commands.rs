//! Filesystem operations implementation (part 2 - command execution)

use std::future::Future;
use wasmtime::component::LinkerInstance;
use wasmtime::StoreContextMut;

use theater::actor::store::ActorStore;


use crate::command_execution::{execute_command, execute_nix_command};
use crate::events::{CommandResult, FilesystemEventData};
use crate::path_validation::resolve_and_validate_path;
use crate::FilesystemHandler;

pub fn setup_execute_command(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()>
{
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();
    let allowed_commands = handler.allowed_commands.clone();

    interface
        .func_wrap_async(
            "execute-command",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_dir, command, args): (String, String, Vec<String>)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<CommandResult, String>,)>> + Send> {
                // Validate command if whitelist is configured
                if let Some(allowed) = &allowed_commands {
                    if !allowed.contains(&command) {
                        return Box::new(async move {
                            Ok((Err(format!("Command '{}' not in allowed list", command)),))
                        });
                    }
                }

                // RESOLVE AND VALIDATE PATH
                let dir_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
                    &requested_dir,
                    "execute",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        return Box::new(async move {
                            Ok((Err(format!("Permission denied: {}", e)),))
                        });
                    }
                };

                // Record command execution event
                
                let args_refs: Vec<String> = args.clone();
                let base_path = filesystem_handler.path().clone();
                let command_clone = command.clone();

                Box::new(async move {
                    match execute_command(
                        base_path,
                        &dir_path,
                        &command_clone,
                        &args_refs.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                    )
                    .await
                    {
                        Ok(result) => {
                            // Record successful
                                                        Ok((Ok(result),))
                        }
                        Err(e) => Ok((Err(e.to_string()),)),
                    }
                })
            },
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap execute-command function: {}", e))?;

    Ok(())
}

pub fn setup_execute_nix_command(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()>
{
    let filesystem_handler = handler.clone();
    let permissions = handler.permissions.clone();

    interface
        .func_wrap_async(
            "execute-nix-command",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (requested_dir, command): (String, String)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<CommandResult, String>,)>> + Send> {
                // RESOLVE AND VALIDATE PATH
                let dir_path = match resolve_and_validate_path(
                    filesystem_handler.path(),
                    &requested_dir,
                    "execute",
                    &permissions,
                ) {
                    Ok(path) => path,
                    Err(e) => {
                        return Box::new(async move {
                            Ok((Err(format!("Permission denied: {}", e)),))
                        });
                    }
                };

                // Record nix command execution event
                
                let base_path = filesystem_handler.path().clone();
                let command_clone = command.clone();

                Box::new(async move {
                    match execute_nix_command(base_path, &dir_path, &command_clone).await {
                        Ok(result) => {
                            // Record successful execution
                                                        Ok((Ok(result),))
                        }
                        Err(e) => Ok((Err(e.to_string()),)),
                    }
                })
            },
        )
        .map_err(|e| anyhow::anyhow!("Failed to wrap execute-nix-command function: {}", e))?;

    Ok(())
}
