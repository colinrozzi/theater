use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info, warn};

use crate::{error::CliError, output::formatters::ProjectCreated, templates, CommandContext};

#[derive(Debug, Parser)]
pub struct CreateArgs {
    /// Name of the new actor project
    #[arg(required = true)]
    pub name: String,

    /// Template to use for the new actor
    #[arg(short, long, default_value = "basic")]
    pub template: String,

    /// Output directory to create the project in
    #[arg(short, long)]
    pub output_dir: Option<PathBuf>,

    /// Skip automatic dependency fetching
    #[arg(long)]
    pub skip_deps: bool,

    /// Skip automatic cargo component check
    #[arg(long)]
    pub skip_component_check: bool,
}

/// Execute the create command asynchronously (modernized)
pub async fn execute_async(args: &CreateArgs, ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Creating new actor project: {}", args.name);
    debug!("Using template: {}", args.template);

    // Check if the name is valid
    if !is_valid_project_name(&args.name) {
        return Err(CliError::invalid_input(
            "project_name",
            &args.name,
            "Project names must only contain alphanumeric characters, hyphens, and underscores",
        ));
    }

    // Get the output directory
    let output_dir = match &args.output_dir {
        Some(dir) => dir.clone(),
        None => std::env::current_dir()
            .map_err(|e| CliError::file_operation_failed("get current directory", ".", e))?,
    };

    debug!("Output directory: {}", output_dir.display());

    // Get available templates
    let templates_list = templates::available_templates().map_err(|e| {
        CliError::file_operation_failed(
            "load templates",
            "templates directory",
            e,
        )
    })?;

    // Check if the template exists
    if !templates_list.contains_key(&args.template) {
        let available_templates: Vec<String> = templates_list.keys().cloned().collect();
        return Err(CliError::template_not_found(
            &args.template,
            available_templates,
        ));
    }

    // Create the project
    let project_path = output_dir.join(&args.name);
    
    // Check if directory already exists
    if project_path.exists() {
        return Err(CliError::file_operation_failed(
            "create project",
            project_path.display().to_string(),
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists, 
                "Directory already exists"
            ),
        ));
    }

    // Step 1: Create project from template
    println!("Creating project structure...");
    templates::create_project(&args.template, &args.name, &project_path).map_err(|e| {
        CliError::file_operation_failed(
            "create project",
            project_path.display().to_string(),
            e,
        )
    })?;
    println!("✅ Project created from '{}' template", args.template);

    // Step 2: Check for required tools
    if !args.skip_component_check {
        check_cargo_component()?;
    }

    // Step 3: Fetch WIT dependencies
    if !args.skip_deps {
        println!("\nFetching WIT dependencies...");
        fetch_wit_dependencies(&project_path)?;
    }

    // Step 4: Try to build the project to validate everything works
    if !args.skip_deps && !args.skip_component_check {
        println!("\nBuilding project...");
        match build_project(&project_path) {
            Ok(_) => println!("✅ Build successful"),
            Err(e) => {
                warn!("⚠️  Project created but initial build failed: {}", e);
                warn!("You may need to run 'cargo component build' manually");
            }
        }
    }

    // Add conclusion message
    println!("\nProject '{}' created successfully!", args.name);

    // Create success result and output
    let mut build_instructions = vec![
        format!("cd {}", args.name),
    ];

    if args.skip_deps {
        build_instructions.push("wkg wit fetch".to_string());
    }
    
    build_instructions.extend(vec![
        "cargo component build --release".to_string(),
        "theater start manifest.toml".to_string(),
    ]);

    let result = ProjectCreated {
        name: args.name.clone(),
        template: args.template.clone(),
        path: project_path,
        build_instructions,
    };

    ctx.output.output(&result, None)?;
    Ok(())
}

/// Check if cargo component is installed
fn check_cargo_component() -> Result<(), CliError> {
    debug!("Checking for cargo component...");
    
    let output = Command::new("cargo")
        .args(&["component", "--version"])
        .output()
        .map_err(|e| {
            CliError::MissingTool {
                tool: "cargo component".to_string(),
                install_command: "cargo install cargo-component".to_string(),
            }
        })?;

    if !output.status.success() {
        return Err(CliError::MissingTool {
            tool: "cargo component".to_string(),
            install_command: "cargo install cargo-component".to_string(),
        });
    }

    let version = String::from_utf8_lossy(&output.stdout);
    info!("✅ cargo component found: {}", version.trim());
    Ok(())
}

/// Fetch WIT dependencies using wkg
fn fetch_wit_dependencies(project_path: &PathBuf) -> Result<(), CliError> {
    // First try wkg wit fetch with streaming output
    let child = Command::new("wkg")
        .args(&["wit", "fetch"])
        .current_dir(project_path)
        .spawn();

    match child {
        Ok(mut child) => {
            let status = child.wait().map_err(|e| {
                CliError::BuildFailed {
                    output: format!("Failed to wait for wkg wit fetch: {}", e),
                }
            })?;

            if status.success() {
                println!("✅ Dependencies fetched");
                Ok(())
            } else {
                warn!("wkg wit fetch failed, trying alternative methods...");
                try_wasm_tools_fetch(project_path)
            }
        }
        Err(_) => {
            warn!("wkg not found, trying alternative methods...");
            try_wasm_tools_fetch(project_path)
        }
    }
}

/// Try using wasm-tools or other methods to fetch dependencies
fn try_wasm_tools_fetch(project_path: &PathBuf) -> Result<(), CliError> {
    // For now, just warn the user and provide instructions
    warn!("⚠️  Could not automatically fetch WIT dependencies");
    warn!("Please run one of the following manually:");
    warn!("  - wkg wit fetch  (if you have wkg installed)");
    warn!("  - Or manually download theater:simple WIT files to wit/deps/theater-simple/");
    
    // Don't fail the creation, just warn
    Ok(())
}

/// Build the project to validate it works
fn build_project(project_path: &PathBuf) -> Result<(), CliError> {
    debug!("Building project at {}", project_path.display());
    
    let mut child = Command::new("cargo")
        .args(&["component", "build", "--target", "wasm32-unknown-unknown", "--release"])
        .current_dir(project_path)
        .spawn()
        .map_err(|e| {
            CliError::BuildFailed {
                output: format!("Failed to execute cargo component build: {}", e),
            }
        })?;

    let status = child.wait().map_err(|e| {
        CliError::BuildFailed {
            output: format!("Failed to wait for cargo component build: {}", e),
        }
    })?;

    if !status.success() {
        return Err(CliError::BuildFailed {
            output: "Build failed - see output above for details".to_string(),
        });
    }

    Ok(())
}

fn is_valid_project_name(name: &str) -> bool {
    // Check that the name only contains alphanumeric characters, hyphens, and underscores
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}
