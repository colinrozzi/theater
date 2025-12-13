//! Command execution functionality for filesystem handler

use std::path::{Path, PathBuf};
use tokio::process::Command as AsyncCommand;
use tracing::info;

use theater::events::filesystem::{CommandError, CommandResult, CommandSuccess};

/// Execute a command in a directory with the given arguments
pub async fn execute_command(
    allowed_path: PathBuf,
    dir: &Path,
    cmd: &str,
    args: &[&str],
) -> anyhow::Result<CommandResult> {
    // Validate that the directory is within our allowed path
    if !dir.starts_with(&allowed_path) {
        return Ok(CommandResult::Error(CommandError {
            message: "Directory not within allowed path".to_string(),
        }));
    }

    if cmd != "nix" {
        return Ok(CommandResult::Error(CommandError {
            message: "Command not allowed".to_string(),
        }));
    }

    if args
        != &[
            "develop",
            "--command",
            "bash",
            "-c",
            "cargo component build --target wasm32-unknown-unknown --release",
        ]
        && args != &["flake", "init"]
    {
        info!("Args not allowed");
        info!("{:?}", args);
        return Ok(CommandResult::Error(CommandError {
            message: "Args not allowed".to_string(),
        }));
    }

    info!("Executing command: {} {:?}", cmd, args);

    // Execute the command
    let output = AsyncCommand::new(cmd)
        .current_dir(dir)
        .args(args)
        .output()
        .await?;

    info!("Command executed");
    info!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    info!("stderr: {}", String::from_utf8_lossy(&output.stderr));
    info!("exit code: {}", output.status.code().unwrap());

    Ok(CommandResult::Success(CommandSuccess {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    }))
}

/// Execute a nix development command
pub async fn execute_nix_command(
    allowed_path: PathBuf,
    dir: &Path,
    command: &str,
) -> anyhow::Result<CommandResult> {
    execute_command(allowed_path, dir, "nix", &["develop", "--command", command]).await
}
