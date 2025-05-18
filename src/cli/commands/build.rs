use anyhow::{anyhow, Context, Result};
use clap::Parser;
use console::style;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, error};

// Import Theater types for working with manifests
use theater::config::ManifestConfig;

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

pub fn execute(args: &BuildArgs, verbose: bool, json: bool) -> Result<()> {
    let project_dir = if args.project_dir.is_absolute() {
        args.project_dir.clone()
    } else {
        std::env::current_dir()?.join(&args.project_dir)
    };

    debug!("Building actor in directory: {}", project_dir.display());
    debug!("Release mode: {}", args.release);
    debug!("Clean build: {}", args.clean);

    // Check if the directory contains a Cargo.toml file
    let cargo_toml_path = project_dir.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        return Err(anyhow!(
            "Not a Rust project directory (Cargo.toml not found): {}",
            project_dir.display()
        ));
    }

    // Get the package name from Cargo.toml
    let package_name = get_package_name(&cargo_toml_path)?;
    
    // Check for manifest.toml
    let manifest_path = project_dir.join("manifest.toml");
    let manifest_exists = manifest_path.exists();
    if !manifest_exists && !json {
        println!(
            "{} No manifest.toml found in project directory. Will build the WebAssembly component, but you'll need to create a manifest to deploy it.",
            style("ℹ").blue().bold()
        );
    }

    // Check if cargo-component is installed
    if !is_cargo_component_installed() {
        return Err(anyhow!(
            "cargo-component is not installed. Please install it with 'cargo install cargo-component'."
        ));
    }

    // Perform cleaning if requested
    if args.clean {
        if !json {
            println!("Cleaning build artifacts...");
        }

        // Clean Cargo artifacts
        let mut clean_cmd = Command::new("cargo");
        clean_cmd.arg("clean").current_dir(&project_dir);

        if let Err(e) = run_command_with_output(&mut clean_cmd, verbose) {
            error!("Failed to clean cargo artifacts: {}", e);
            // Continue anyway, as this is not fatal
        }
    }

    // Build the WebAssembly component using cargo-component
    if !json {
        println!(
            "Building WebAssembly component for actor in {}...",
            project_dir.display()
        );
    }

    // Execute cargo component build
    let mut build_cmd = Command::new("cargo");
    build_cmd.args(["component", "build", "--target", "wasm32-unknown-unknown"]);

    if args.release {
        build_cmd.arg("--release");
    }

    build_cmd.current_dir(&project_dir);

    // Run the build command and capture output
    let (status, stdout, stderr) = match run_command_with_output(&mut build_cmd, verbose) {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to execute cargo component build: {}", e);
            if !json {
                println!("{} Build failed: {}", style("✗").red().bold(), e);
            } else {
                let output = serde_json::json!({
                    "success": false,
                    "project_dir": project_dir.display().to_string(),
                    "error": format!("Failed to execute cargo component build: {}", e)
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            return Err(anyhow!("Failed to execute cargo component build: {}", e));
        }
    };

    // Handle build failures
    if !status.success() {
        error!("Cargo component build failed with status: {}", status);
        
        if !json {
            println!("{} Build failed with errors:\n", style("✗").red().bold());
            eprintln!("{}", stderr);
        } else {
            let output = serde_json::json!({
                "success": false,
                "project_dir": project_dir.display().to_string(),
                "error": "Build failed"
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }

        return Err(anyhow!("Cargo component build failed"));
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
        error!("Built WASM file not found at expected path: {}", wasm_path.display());
        return Err(anyhow!(
            "Built WASM file not found at expected path: {}",
            wasm_path.display()
        ));
    }

    // Update the manifest.toml with the new component path if it exists
    if manifest_exists {
        let manifest_content =
            fs::read_to_string(&manifest_path).context("Failed to read manifest.toml")?;

        let mut manifest: ManifestConfig =
            toml::from_str(&manifest_content).context("Failed to parse manifest.toml")?;

        // Update the component path - use absolute path to the wasm file
        manifest.component_path = wasm_path.to_string_lossy().to_string();

        // Write the updated manifest
        let updated_manifest =
            toml::to_string(&manifest).context("Failed to serialize manifest.toml")?;

        fs::write(&manifest_path, updated_manifest)
            .context("Failed to write updated manifest.toml")?;
            
        info!("Updated manifest with component path: {}", wasm_path.display());
    }

    if !json {
        println!(
            "{} Successfully built WebAssembly component: {}",
            style("✓").green().bold(),
            style(wasm_path.display()).cyan()
        );

        // Instructions for deployment if manifest exists
        if manifest_exists {
            println!("\nTo deploy your actor:");
            println!("  theater start {}", manifest_path.display());
        } else {
            println!("\nTo create a manifest for your actor:");
            println!(
                "  theater create-manifest {} --component-path {}",
                project_dir.display(),
                wasm_path.display()
            );
        }
    } else {
        let output = serde_json::json!({
            "success": true,
            "project_dir": project_dir.display().to_string(),
            "wasm_path": wasm_path.to_string_lossy().to_string(),
            "manifest_exists": manifest_exists,
            "manifest_path": manifest_path.display().to_string()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

/// Run a command and return the status, stdout, and stderr
fn run_command_with_output(
    cmd: &mut Command,
    verbose: bool,
) -> Result<(std::process::ExitStatus, String, String)> {
    debug!("Running command: {:?}", cmd);

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
        let output = cmd
            .output()
            .map_err(|e| anyhow!("Failed to execute command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok((output.status, stdout, stderr))
    }
}

/// Check if cargo-component is installed
fn is_cargo_component_installed() -> bool {
    let output = Command::new("cargo")
        .args(["--list"])
        .output();
        
    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("component")
        },
        Err(_) => false,
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
