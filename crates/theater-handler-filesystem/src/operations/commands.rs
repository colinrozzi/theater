//! Filesystem operations implementation (part 2 - command execution)

use std::future::Future;
use wasmtime::component::LinkerInstance;
use wasmtime::StoreContextMut;

use theater::actor::store::ActorStore;
use theater::events::filesystem::{CommandResult, FilesystemEventData};
use theater::events::{ChainEventData, EventData};

use crate::command_execution::{execute_command, execute_nix_command};
use crate::path_validation::resolve_and_validate_path;
use crate::FilesystemHandler;

pub fn setup_execute_command(
    handler: &FilesystemHandler,
    interface: &mut LinkerInstance<ActorStore>,
) -> anyhow::Result<()> {
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
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/execute-command".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::CommandExecuted {
                        directory: requested_dir.clone(),
                        command: command.clone(),
                        args: args.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Executing command '{}' in directory '{}'",
                        command, requested_dir
                    )),
                });

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
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/filesystem/command-result".to_string(),
                                data: EventData::Filesystem(FilesystemEventData::CommandCompleted {
                                    result: result.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some("Command completed".to_string()),
                            });
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
) -> anyhow::Result<()> {
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
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/filesystem/execute-nix-command".to_string(),
                    data: EventData::Filesystem(FilesystemEventData::NixCommandExecuted {
                        directory: requested_dir.clone(),
                        command: command.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Executing nix command '{}' in directory '{}'",
                        command, requested_dir
                    )),
                });

                let base_path = filesystem_handler.path().clone();
                let command_clone = command.clone();

                Box::new(async move {
                    match execute_nix_command(base_path, &dir_path, &command_clone).await {
                        Ok(result) => {
                            // Record successful execution
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/filesystem/nix-command-result".to_string(),
                                data: EventData::Filesystem(FilesystemEventData::CommandCompleted {
                                    result: result.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some("Nix command completed".to_string()),
                            });
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
