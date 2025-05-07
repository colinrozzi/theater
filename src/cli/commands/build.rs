use anyhow::{anyhow, Context, Result};
use clap::Parser;
use console::style;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime};
use tracing::{debug, error};

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

    /// Force rebuild even if the component is up to date
    #[arg(long, default_value = "false")]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    pub last_build_time: Option<SystemTime>,
    pub build_status: BuildStatus,
    pub component_hash: Option<String>,
    pub build_log: Option<String>,
    pub build_duration: Option<u64>,
    pub component_size: Option<u64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BuildStatus {
    NotBuilt,
    Building,
    Success,
    Failed,
}

impl std::fmt::Display for BuildStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildStatus::NotBuilt => write!(f, "Not Built"),
            BuildStatus::Building => write!(f, "Building"),
            BuildStatus::Success => write!(f, "Success"),
            BuildStatus::Failed => write!(f, "Failed"),
        }
    }
}

impl Default for BuildInfo {
    fn default() -> Self {
        Self {
            last_build_time: None,
            build_status: BuildStatus::NotBuilt,
            component_hash: None,
            build_log: None,
            build_duration: None,
            component_size: None,
            error_message: None,
        }
    }
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
    debug!("Force rebuild: {}", args.force);

    // Check if the directory contains a Cargo.toml file
    let cargo_toml_path = project_dir.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        return Err(anyhow!(
            "Not a Rust project directory (Cargo.toml not found): {}",
            project_dir.display()
        ));
    }

    // Check if the directory contains a flake.nix file
    let flake_nix_path = project_dir.join("flake.nix");
    if !flake_nix_path.exists() {
        if !json {
            println!(
                "{} No flake.nix found in project directory. Creating a default flake.nix file.",
                style("⚠").yellow().bold()
            );
        }

        // Get the package name from Cargo.toml
        let package_name = get_package_name(&cargo_toml_path)?;

        // Create a default flake.nix file
        create_default_flake_nix(&project_dir, &package_name)?;
    }

    // Check for manifest.toml
    let manifest_path = project_dir.join("manifest.toml");
    let manifest_exists = manifest_path.exists();
    if !manifest_exists {
        if !json {
            println!(
                "{} No manifest.toml found in project directory. Will build the WebAssembly component, but you'll need to create a manifest to deploy it.",
                style("⚠").yellow().bold()
            );
        }
    }

    // Create build_info directory if it doesn't exist
    let build_info_dir = project_dir.join(".build_info");
    if !build_info_dir.exists() {
        fs::create_dir_all(&build_info_dir)?;
    }

    // Create log file
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let log_file = build_info_dir.join(format!("build_{}.log", timestamp));
    let log_file_path = log_file.to_string_lossy().to_string();

    // Start timing the build
    let build_start = Instant::now();

    // Update status to building
    let status_file = build_info_dir.join("status");
    fs::write(&status_file, "BUILDING")?;

    if args.clean {
        // Run cargo clean and nix clean
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

        // Clean Nix store artifacts if appropriate
        /*
        let mut nix_clean_cmd = Command::new("nix").args(["store", "gc"]);
        if let Err(e) = run_command_with_output(&mut nix_clean_cmd, verbose) {
            error!("Failed to clean nix store: {}", e);
            // Continue anyway, as this is not fatal
        }
        */
        println!("Nix store cleaning is not implemented yet, but you can do it manually with `nix store gc`.");
    }

    // Build the WebAssembly component using nix
    if !json {
        println!(
            "Building WebAssembly component for actor in {}...",
            project_dir.display()
        );
    }

    // Execute nix build
    let mut nix_cmd = Command::new("nix");
    nix_cmd.args(["build", "--no-link", "--print-out-paths"]);

    if args.force {
        nix_cmd.arg("--no-cache");
    }

    nix_cmd.current_dir(&project_dir);

    // Run the nix build command and capture output
    let (nix_status, nix_stdout, nix_stderr) = match run_command_with_output(&mut nix_cmd, verbose)
    {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to execute nix build command: {}", e);

            // Create a failure log
            let mut log_content = format!("=== Build Log for {} ===\n", project_dir.display());
            log_content.push_str(&format!("Date: {}\n", timestamp));
            log_content.push_str("Builder: nix\n");
            log_content.push_str(&format!(
                "Duration: {} seconds\n\n",
                build_start.elapsed().as_secs()
            ));
            log_content.push_str(&format!(
                "ERROR: Failed to execute nix build command: {}\n",
                e
            ));

            // Write the log file
            fs::write(&log_file, log_content)?;

            // Update status
            fs::write(&status_file, "FAILED")?;

            // Create build_info
            let build_info = BuildInfo {
                last_build_time: Some(SystemTime::now()),
                build_status: BuildStatus::Failed,
                component_hash: None,
                build_log: Some(log_file_path),
                build_duration: Some(build_start.elapsed().as_secs()),
                component_size: None,
                error_message: Some(format!("Failed to execute nix build command: {}", e)),
            };

            // Write build_info
            let build_info_json = serde_json::to_string_pretty(&build_info)?;
            fs::write(build_info_dir.join("build_info.json"), build_info_json)?;

            return Err(anyhow!("Failed to execute nix build command: {}", e));
        }
    };

    // Get additional logs if build failed
    let (full_stderr, error_message) = if !nix_status.success() {
        error!("Nix build failed with status: {}", nix_status);

        // Try to extract the derivation path for more detailed logs
        let drv_path = nix_stderr
            .lines()
            .find(|line| line.contains("nix-store -l"))
            .and_then(|line| {
                let parts: Vec<&str> = line.split('\'').collect();
                if parts.len() >= 2 {
                    Some(
                        parts[1]
                            .trim()
                            .strip_prefix("nix-store -l ")
                            .unwrap_or(parts[1])
                            .trim(),
                    )
                } else {
                    None
                }
            });

        let full_logs = if let Some(path) = drv_path {
            debug!("Getting full logs from: {}", path);
            match Command::new("nix-store").args(["-l", path]).output() {
                Ok(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout).to_string()
                }
                _ => nix_stderr.clone(),
            }
        } else {
            nix_stderr.clone()
        };

        // Extract a concise error message
        let error_msg = full_logs
            .lines()
            .find(|line| line.contains("error:"))
            .map(|line| line.trim().to_string());

        (full_logs, error_msg)
    } else {
        (nix_stderr, None)
    };

    // Update build info and status
    if !nix_status.success() {
        // Write build logs
        let mut log_content = format!("=== Build Log for {} ===\n", project_dir.display());
        log_content.push_str(&format!("Date: {}\n", timestamp));
        log_content.push_str("Builder: nix\n");
        log_content.push_str(&format!(
            "Duration: {} seconds\n\n",
            build_start.elapsed().as_secs()
        ));

        log_content.push_str("=== STDOUT ===\n");
        log_content.push_str(&nix_stdout);

        log_content.push_str("\n=== STDERR ===\n");
        log_content.push_str(&full_stderr);

        log_content.push_str(&format!("\n=== Exit Status: {} ===\n", nix_status));

        fs::write(&log_file, log_content)?;

        // Update status
        fs::write(&status_file, "FAILED")?;

        // Create build_info
        let build_info = BuildInfo {
            last_build_time: Some(SystemTime::now()),
            build_status: BuildStatus::Failed,
            component_hash: None,
            build_log: Some(log_file_path.clone()),
            build_duration: Some(build_start.elapsed().as_secs()),
            component_size: None,
            error_message: error_message.clone(),
        };

        // Write build_info
        let build_info_json = serde_json::to_string_pretty(&build_info)?;
        fs::write(build_info_dir.join("build_info.json"), build_info_json)?;

        if !json {
            println!("{} Build failed with errors:\n", style("✗").red().bold());
            // Just print the error directly - it already has ANSI color codes
            eprintln!("{}", full_stderr);
        } else {
            let output = serde_json::json!({
                "success": false,
                "project_dir": project_dir.display().to_string(),
                "error": error_message.unwrap_or_else(|| "Build failed".to_string()),
                "log_file": log_file_path,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }

        return Err(anyhow!("Nix build failed with status: {}", nix_status));
    }

    // Get the output path from stdout
    let nix_store_path = nix_stdout.trim();

    if nix_store_path.is_empty() {
        error!("Failed to determine nix store path");
        fs::write(&status_file, "FAILED")?;
        return Err(anyhow!("Failed to determine nix store path"));
    }

    // Construct the WASM file path
    // The filename in the nix store will match the actor name (with hyphens)
    let wasm_file_name = "component.wasm";
    let wasm_path = format!("{}/lib/{}", nix_store_path, wasm_file_name);

    // Check if the WASM file exists
    if !Path::new(&wasm_path).exists() {
        error!("Built WASM file not found at expected path: {}", wasm_path);
        fs::write(&status_file, "FAILED")?;
        return Err(anyhow!(
            "Built WASM file not found at expected path: {}",
            wasm_path
        ));
    }

    // Calculate component hash and size
    let component_hash = calculate_file_hash(&wasm_path)?;
    let component_size = get_file_size(&wasm_path)?;

    // Update the manifest.toml with the new component path if it exists
    if manifest_exists {
        let manifest_content =
            fs::read_to_string(&manifest_path).context("Failed to read manifest.toml")?;

        let mut manifest: ManifestConfig =
            toml::from_str(&manifest_content).context("Failed to parse manifest.toml")?;

        // Update the component path
        manifest.component_path = wasm_path.clone();

        // Write the updated manifest
        let updated_manifest =
            toml::to_string(&manifest).context("Failed to serialize manifest.toml")?;

        fs::write(&manifest_path, updated_manifest)
            .context("Failed to write updated manifest.toml")?;
    }

    // Write build logs
    let mut log_content = format!("=== Build Log for {} ===\n", project_dir.display());
    log_content.push_str(&format!("Date: {}\n", timestamp));
    log_content.push_str("Builder: nix\n");
    log_content.push_str(&format!(
        "Duration: {} seconds\n\n",
        build_start.elapsed().as_secs()
    ));

    log_content.push_str("=== STDOUT ===\n");
    log_content.push_str(&nix_stdout);

    log_content.push_str("\n=== STDERR ===\n");
    log_content.push_str(&full_stderr);

    log_content.push_str(&format!("\n=== Exit Status: {} ===\n", nix_status));
    log_content.push_str(&format!("\n=== Component Hash: {} ===\n", component_hash));
    log_content.push_str(&format!(
        "\n=== Component Size: {} bytes ===\n",
        component_size
    ));
    log_content.push_str(&format!("\n=== Component Path: {} ===\n", wasm_path));

    fs::write(&log_file, log_content)?;

    // Update status
    fs::write(&status_file, "SUCCESS")?;

    // Create build_info
    let build_info = BuildInfo {
        last_build_time: Some(SystemTime::now()),
        build_status: BuildStatus::Success,
        component_hash: Some(component_hash.clone()),
        build_log: Some(log_file_path),
        build_duration: Some(build_start.elapsed().as_secs()),
        component_size: Some(component_size),
        error_message: None,
    };

    // Write build_info
    let build_info_json = serde_json::to_string_pretty(&build_info)?;
    fs::write(build_info_dir.join("build_info.json"), build_info_json)?;

    if !json {
        println!(
            "{} Successfully built WebAssembly component: {}",
            style("✓").green().bold(),
            style(&wasm_path).cyan()
        );

        // Instructions for deployment if manifest exists
        if manifest_exists {
            println!("\nTo deploy your actor:");
            println!("  theater deploy {}", manifest_path.display());
        } else {
            println!("\nTo create a manifest for your actor:");
            println!(
                "  theater create-manifest {} --component-path {}",
                project_dir.display(),
                wasm_path
            );
        }
    } else {
        let output = serde_json::json!({
            "success": true,
            "project_dir": project_dir.display().to_string(),
            "wasm_path": wasm_path,
            "manifest_exists": manifest_exists,
            "manifest_path": manifest_path.display().to_string(),
            "component_hash": component_hash,
            "component_size": component_size,
            "build_duration": build_start.elapsed().as_secs()
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

/// Create a default flake.nix file for the actor
fn create_default_flake_nix(project_dir: &Path, package_name: &str) -> Result<()> {
    let default_flake = format!(
        r#"{{
  description = "{0} - A Theater Actor";

  inputs = {{
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {{
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    }};
  }};

  outputs = {{ self, nixpkgs, flake-utils, rust-overlay }}:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {{
          inherit system overlays;
        }};

        rustToolchain = pkgs.rust-bin.stable."1.82.0".default.override {{
          targets = [ "wasm32-unknown-unknown" ];
          extensions = [ "rust-src" ];
        }};
        
        crateNameForWasm = "{1}";
        wasmFileName = "${{crateNameForWasm}}.wasm";

      in {{
        packages.default = pkgs.stdenv.mkDerivation {{
          name = "{1}";
          src = ./.;
          
          nativeBuildInputs = with pkgs; [
            rustToolchain
            pkg-config
            openssl
          ];

          buildPhase = ''
            export RUSTUP_TOOLCHAIN=${{rustToolchain}}
            export CARGO_HOME=$(mktemp -d cargo-home.XXX)
            
            cargo component build --release --target wasm32-unknown-unknown
            
            mkdir -p $out/lib
            cp target/wasm32-unknown-unknown/release/${{wasmFileName}} $out/lib/
          '';
          
          installPhase = ''
            # Nothing to do here as we copied the files in buildPhase
          '';
        }};
      }}
    );
}}
"#,
        package_name,                   // {0} - Description
        package_name.replace('_', "-")  // {1} - crate name for wasm (ensure it uses hyphens)
    );

    fs::write(project_dir.join("flake.nix"), default_flake)?;

    Ok(())
}

/// Calculate file hash (MD5)
fn calculate_file_hash(file_path: &str) -> Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(file_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let digest = md5::compute(&buffer);
    Ok(format!("{:x}", digest))
}

/// Get file size in bytes
fn get_file_size(file_path: &str) -> Result<u64> {
    let metadata = std::fs::metadata(file_path)?;
    Ok(metadata.len())
}
