use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

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

    // Check if the directory contains a manifest.toml file
    let manifest_path = project_dir.join("manifest.toml");
    if !manifest_path.exists() {
        if !json {
            println!(
                "{} No manifest.toml found in project directory. Will build the WebAssembly component, but you'll need to create a manifest to deploy it.",
                style("⚠").yellow().bold()
            );
        }
    }

    if args.clean {
        // Run cargo clean
        if !json {
            println!("Cleaning target directory...");
        }
        
        let status = run_command(
            Command::new("cargo")
                .arg("clean")
                .current_dir(&project_dir),
            verbose,
        )?;
        
        if !status.success() {
            return Err(anyhow!("Failed to clean target directory"));
        }
    }

    // Build the WebAssembly component
    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd.arg("build");
    
    if args.release {
        cargo_cmd.arg("--release");
    }
    
    cargo_cmd
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(&project_dir);

    if !json {
        println!(
            "Building WebAssembly component for actor in {}...",
            project_dir.display()
        );
    }

    let status = run_command(&mut cargo_cmd, verbose)?;
    if !status.success() {
        return Err(anyhow!("Failed to build WebAssembly component"));
    }

    // Get the output path (assuming the Cargo.toml name matches the package name)
    let package_name = get_package_name(&cargo_toml_path)?;
    let target_dir = if args.release {
        "target/wasm32-unknown-unknown/release"
    } else {
        "target/wasm32-unknown-unknown/debug"
    };
    
    let wasm_file = format!("{}.wasm", package_name);
    let wasm_path = project_dir.join(target_dir).join(&wasm_file);

    if !wasm_path.exists() {
        return Err(anyhow!(
            "WebAssembly component not found at expected location: {}",
            wasm_path.display()
        ));
    }

    if !json {
        println!(
            "{} Successfully built WebAssembly component: {}",
            style("✓").green().bold(),
            style(wasm_path.display().to_string()).cyan()
        );

        // Instructions for deployment if manifest exists
        if manifest_path.exists() {
            println!("\nTo deploy your actor:");
            println!("  theater deploy {}", manifest_path.display());
        }
    } else {
        let output = serde_json::json!({
            "success": true,
            "project_dir": project_dir.display().to_string(),
            "wasm_path": wasm_path.display().to_string(),
            "release": args.release,
            "manifest_exists": manifest_path.exists(),
            "manifest_path": manifest_path.display().to_string()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

/// Run a command with optional verbose output
fn run_command(cmd: &mut Command, verbose: bool) -> Result<std::process::ExitStatus> {
    debug!("Running command: {:?}", cmd);
    
    if verbose {
        // Run with inherited stdout/stderr for verbose output
        cmd.status().map_err(|e| anyhow!("Failed to execute command: {}", e))
    } else {
        // Capture stdout/stderr for normal output
        let output = cmd
            .output()
            .map_err(|e| anyhow!("Failed to execute command: {}", e))?;
        
        if !output.status.success() && !output.stderr.is_empty() {
            info!(
                "Command failed with: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        
        Ok(output.status)
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
                return Ok(name.replace('-', "_")); // Convert kebab-case to snake_case for WASM filename
            }
        }
    }
    
    Err(anyhow!("Could not find package name in Cargo.toml"))
}
