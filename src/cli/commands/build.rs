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
        
        let mut clean_cmd = Command::new("cargo");
        clean_cmd.arg("clean").current_dir(&project_dir);
        
        match run_command_with_output(&mut clean_cmd, verbose) {
            Ok((status, _, stderr)) => {
                if !status.success() {
                    if !stderr.is_empty() {
                        if !json {
                            println!("{} Clean failed with errors:\n", style("✗").red().bold());
                            // Print stderr directly to preserve colors
                            eprint!("{}", stderr);
                        }
                        return Err(anyhow!("Failed to clean target directory"));
                    } else {
                        return Err(anyhow!("Failed to clean target directory"));
                    }
                }
            }
            Err(e) => {
                return Err(anyhow!("Failed to execute cargo clean command: {}", e));
            }
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

    // Run the cargo build command and capture any errors
    match run_command_with_output(&mut cargo_cmd, verbose) {
        Ok((status, stdout, stderr)) => {
            if !status.success() {
                // If there's stderr output, display it with preserved colors
                if !stderr.is_empty() {
                    if !json {
                        println!("{} Build failed with errors:\n", style("✗").red().bold());
                        // Just print the stderr directly - it already has ANSI color codes
                        eprint!("{}", stderr);
                    }
                    return Err(anyhow!("Failed to build WebAssembly component"));
                } else {
                    return Err(anyhow!("Failed to build WebAssembly component"));
                }
            }
            // For non-verbose mode, if there is stdout and we want to show it
            if !verbose && !stdout.is_empty() && false { // Typically we don't want to show stdout
                print!("{}", stdout);
            }
        }
        Err(e) => {
            return Err(anyhow!("Failed to execute cargo build command: {}", e));
        }
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
        // Canonicalize paths for cleaner display
        let normalized_wasm_path = canonicalize_path(&wasm_path)?;
        let normalized_manifest_path = if manifest_path.exists() {
            canonicalize_path(&manifest_path)?
        } else {
            manifest_path.display().to_string()
        };
        
        println!(
            "{} Successfully built WebAssembly component: {}",
            style("✓").green().bold(),
            style(normalized_wasm_path).cyan()
        );

        // Instructions for deployment if manifest exists
        if manifest_path.exists() {
            println!("\nTo deploy your actor:");
            println!("  theater deploy {}", normalized_manifest_path);
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

/// Run a command and return the status, stdout, and stderr
fn run_command_with_output(cmd: &mut Command, verbose: bool) -> Result<(std::process::ExitStatus, String, String)> {
    debug!("Running command: {:?}", cmd);
    
    // Force ANSI colors for Cargo
    cmd.env("CARGO_TERM_COLOR", "always");
    
    if verbose {
        // For verbose mode, we'll just let cargo output directly to the console
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

/// Canonicalize a path to its absolute, clean form
fn canonicalize_path(path: &Path) -> Result<String> {
    // Use std::fs::canonicalize to get the canonical form of the path
    // This resolves all symbolic links and normalizes the path
    match std::fs::canonicalize(path) {
        Ok(canon_path) => Ok(canon_path.display().to_string()),
        Err(_) => {
            // Fallback to manual normalization if canonicalization fails
            // (which can happen if the file doesn't exist yet)
            let path_str = path.display().to_string();
            
            // Remove redundant path components
            let path_str = path_str
                .replace("/./", "/")  // Replace /./ with /
                .replace("//", "/");  // Replace // with /
                
            // Remove trailing /. if present
            let path_str = if path_str.ends_with("/.") {
                path_str[..path_str.len()-2].to_string()
            } else {
                path_str
            };
            
            Ok(path_str)
        }
    }
}
