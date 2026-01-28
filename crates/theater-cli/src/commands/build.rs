use anyhow::{anyhow, Result};
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, error, info};

use crate::{error::CliError, output::formatters::BuildResult, CommandContext};
use theater::config::actor_manifest::ManifestConfig;

#[derive(Debug, Parser)]
pub struct BuildArgs {
    /// Directory containing the actor project
    #[arg(default_value = ".")]
    pub project_dir: PathBuf,

    /// Build in release mode
    #[arg(short, long, default_value = "true")]
    pub release: bool,

    /// Clean the target directory before building
    #[arg(short, long, default_value = "false")]
    pub clean: bool,
}

/// Execute the build command asynchronously (modernized)
pub async fn execute_async(args: &BuildArgs, ctx: &CommandContext) -> Result<(), CliError> {
    let project_dir = if args.project_dir.is_absolute() {
        args.project_dir.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| CliError::file_operation_failed("get current directory", ".", e))?
            .join(&args.project_dir)
    };

    debug!("Building actor in directory: {}", project_dir.display());
    debug!("Release mode: {}", args.release);
    debug!("Clean build: {}", args.clean);

    // Check if the directory contains a Cargo.toml file
    let cargo_toml_path = project_dir.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        return Err(CliError::invalid_manifest(format!(
            "Not a Rust project directory (Cargo.toml not found): {}",
            project_dir.display()
        )));
    }

    // Get the package name from Cargo.toml
    let package_name = get_package_name(&cargo_toml_path)
        .map_err(|e| CliError::invalid_manifest(format!("Failed to parse Cargo.toml: {}", e)))?;

    // Check for manifest.toml
    let manifest_path = project_dir.join("manifest.toml");
    let manifest_exists = manifest_path.exists();

    // Perform cleaning if requested
    if args.clean {
        debug!("Cleaning build artifacts...");
        let mut clean_cmd = Command::new("cargo");
        clean_cmd.arg("clean").current_dir(&project_dir);

        if let Err(e) = run_command_with_output(&mut clean_cmd, ctx.verbose) {
            error!("Failed to clean cargo artifacts: {}", e);
            // Continue anyway, as this is not fatal
        }
    }

    // Build the WebAssembly module
    debug!(
        "Building WebAssembly module for actor in {}...",
        project_dir.display()
    );

    // Execute cargo build with WASM target
    let mut build_cmd = Command::new("cargo");
    build_cmd.args(["build", "--target", "wasm32-unknown-unknown"]);

    if args.release {
        build_cmd.arg("--release");
    }

    build_cmd.current_dir(&project_dir);

    // Run the build command and capture output
    let (status, stdout, stderr) =
        run_command_with_output(&mut build_cmd, ctx.verbose).map_err(|e| {
            CliError::build_failed(format!("Failed to execute cargo build: {}", e))
        })?;

    // Handle build failures
    if !status.success() {
        let error_details = if stderr.is_empty() { stdout } else { stderr };
        return Err(CliError::build_failed(format!(
            "Cargo build failed:\\n{}",
            error_details
        )));
    }

    // Construct the path to the built wasm file
    let build_type = if args.release { "release" } else { "debug" };
    let wasm_file_name = format!("{}.wasm", package_name.replace('-', "_"));
    let wasm_path = project_dir
        .join("target/wasm32-unknown-unknown")
        .join(build_type)
        .join(&wasm_file_name);

    // Validate the wasm file exists
    if !wasm_path.exists() {
        return Err(CliError::build_failed(format!(
            "Built WASM file not found at expected path: {}",
            wasm_path.display()
        )));
    }

    // Update the manifest.toml with the new component path if it exists
    if manifest_exists {
        let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| {
            CliError::file_operation_failed(
                "read manifest.toml",
                manifest_path.display().to_string(),
                e,
            )
        })?;

        let mut manifest: ManifestConfig = toml::from_str(&manifest_content).map_err(|e| {
            CliError::invalid_manifest(format!("Failed to parse manifest.toml: {}", e))
        })?;

        // Update the package path - use absolute path to the wasm file
        manifest.package = wasm_path.to_string_lossy().to_string();

        // Write the updated manifest
        let updated_manifest = toml::to_string(&manifest).map_err(|e| {
            CliError::invalid_manifest(format!("Failed to serialize manifest.toml: {}", e))
        })?;

        fs::write(&manifest_path, updated_manifest).map_err(|e| {
            CliError::file_operation_failed(
                "write manifest.toml",
                manifest_path.display().to_string(),
                e,
            )
        })?;

        info!(
            "Updated manifest with component path: {}",
            wasm_path.display()
        );
    }

    // Create build result and output
    let result = BuildResult {
        success: true,
        project_dir,
        wasm_path: Some(wasm_path),
        manifest_exists,
        manifest_path: Some(manifest_path),
        build_type: build_type.to_string(),
        package_name,
        stdout,
        stderr,
    };

    ctx.output.output(&result, None)?;
    Ok(())
}

/// Run a command and return the status, stdout, and stderr
fn run_command_with_output(
    cmd: &mut Command,
    verbose: bool,
) -> Result<(std::process::ExitStatus, String, String)> {
    debug!("Running command: {:?}", cmd);
    cmd.env("RUST_BACKTRACE", "1");
    cmd.env("RUST_COLOR", "always");
    cmd.env("CARGO_TERM_COLOR", "always");

    if verbose {
        // For verbose mode, we'll just let the command output directly to the console
        // with all its colors, and then capture the output separately for the result
        let status = cmd
            .status()
            .map_err(|e| anyhow!("Failed to execute command: {}", e))?;

        // If we're in verbose mode and directly showing output, return empty strings for stdout/stderr
        // since they were already displayed
        Ok((status, String::new(), String::new()))
    } else {
        // For non-verbose mode, capture the output but preserve ANSI color codes
        // Capture the output
        let output = cmd
            .output()
            .map_err(|e| anyhow!("Failed to execute command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok((output.status, stdout, stderr))
    }
}

/// Extract the package name from Cargo.toml
fn get_package_name(cargo_toml_path: &Path) -> Result<String> {
    let cargo_toml = std::fs::read_to_string(cargo_toml_path)?;

    // Simple parse to extract package name
    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with("name") {
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() >= 2 {
                let name = parts[1].trim().trim_matches('"').trim_matches('\'');
                return Ok(name.to_string());
            }
        }
    }

    Err(anyhow!("Could not find package name in Cargo.toml"))
}
