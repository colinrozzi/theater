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

    /// Template to use for the new actor (available: basic, http-server, message-server, supervisor)
    #[arg(short, long, default_value = "basic")]
    pub template: String,

    /// Output directory to create the project in
    #[arg(short, long)]
    pub output_dir: Option<PathBuf>,

    /// Skip automatic dependency fetching
    #[arg(long)]
    pub skip_deps: bool,

    /// Skip automatic build check
    #[arg(long)]
    pub skip_build_check: bool,

    /// Initialize a git repository and make the first commit
    #[arg(long)]
    pub git: bool,

    /// Skip git repository initialization (opposite of --git)
    #[arg(long, conflicts_with = "git")]
    pub no_git: bool,
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
    let templates_list = templates::available_templates()
        .map_err(|e| CliError::file_operation_failed("load templates", "templates directory", e))?;

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
                "Directory already exists",
            ),
        ));
    }

    // Step 1: Create project from template
    println!("Creating project structure...");
    templates::create_project(&args.template, &args.name, &project_path).map_err(|e| {
        CliError::file_operation_failed("create project", project_path.display().to_string(), e)
    })?;
    println!("✅ Project created from '{}' template", args.template);

    // Step 2: Fetch WIT dependencies
    if !args.skip_deps {
        println!("\nFetching WIT dependencies...");
        fetch_wit_dependencies(&project_path)?;
    }

    // Step 3: Try to build the project to validate everything works
    if !args.skip_deps && !args.skip_build_check {
        println!("\nBuilding project...");
        match build_project(&project_path) {
            Ok(_) => println!("✅ Build successful"),
            Err(e) => {
                warn!("⚠️  Project created but initial build failed: {}", e);
                warn!("You may need to run 'cargo build --target wasm32-unknown-unknown --release' manually");
            }
        }
    }

    // Step 5: Initialize git repository if requested
    if args.git || (!args.no_git && should_init_git()) {
        println!("\nInitializing git repository...");
        match init_git_repo(&project_path, &args.name) {
            Ok(_) => println!("✅ Git repository initialized with initial commit"),
            Err(e) => {
                warn!("⚠️  Failed to initialize git repository: {}", e);
                warn!("You can run 'git init' manually if needed");
            }
        }
    }

    // Add conclusion message
    println!("\nProject '{}' created successfully!", args.name);

    // Create success result and output
    let mut build_instructions = vec![format!("cd {}", args.name)];

    if args.skip_deps {
        build_instructions.push("wkg wit fetch".to_string());
    }

    build_instructions.extend(vec![
        "cargo build --target wasm32-unknown-unknown --release".to_string(),
        "theater start manifest.toml".to_string(),
    ]);

    // Add git instructions if git was not initialized
    if !args.git && (args.no_git || !should_init_git()) {
        build_instructions.insert(1, "git init".to_string());
        build_instructions.insert(2, "git add .".to_string());
        build_instructions.insert(3, "git commit -m 'Initial commit'".to_string());
    }

    let result = ProjectCreated {
        name: args.name.clone(),
        template: args.template.clone(),
        path: project_path,
        build_instructions,
    };

    ctx.output.output(&result, None)?;
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
            let status = child.wait().map_err(|e| CliError::BuildFailed {
                output: format!("Failed to wait for wkg wit fetch: {}", e),
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
fn try_wasm_tools_fetch(_project_path: &PathBuf) -> Result<(), CliError> {
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
        .args(&[
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ])
        .current_dir(project_path)
        .spawn()
        .map_err(|e| CliError::BuildFailed {
            output: format!("Failed to execute cargo build: {}", e),
        })?;

    let status = child.wait().map_err(|e| CliError::BuildFailed {
        output: format!("Failed to wait for cargo build: {}", e),
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

/// Determine if we should initialize git by default (when git is available)
fn should_init_git() -> bool {
    // Check if git is available
    Command::new("git")
        .args(&["--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Initialize a git repository and make the first commit
fn init_git_repo(project_path: &PathBuf, project_name: &str) -> Result<(), CliError> {
    debug!("Initializing git repository at {}", project_path.display());

    // Initialize git repository
    let init_output = Command::new("git")
        .args(&["init"])
        .current_dir(project_path)
        .output()
        .map_err(|_e| CliError::MissingTool {
            tool: "git".to_string(),
            install_command: "Install git from https://git-scm.com/".to_string(),
        })?;

    if !init_output.status.success() {
        return Err(CliError::BuildFailed {
            output: format!(
                "Failed to initialize git repository: {}",
                String::from_utf8_lossy(&init_output.stderr)
            ),
        });
    }

    // Add all files
    let add_output = Command::new("git")
        .args(&["add", "."])
        .current_dir(project_path)
        .output()
        .map_err(|e| CliError::BuildFailed {
            output: format!("Failed to add files to git: {}", e),
        })?;

    if !add_output.status.success() {
        return Err(CliError::BuildFailed {
            output: format!(
                "Failed to add files to git: {}",
                String::from_utf8_lossy(&add_output.stderr)
            ),
        });
    }

    // Make initial commit
    let commit_message = format!("Initial commit: Theater actor project '{}'", project_name);
    let commit_output = Command::new("git")
        .args(&["commit", "-m", &commit_message])
        .current_dir(project_path)
        .output()
        .map_err(|e| CliError::BuildFailed {
            output: format!("Failed to make initial commit: {}", e),
        })?;

    if !commit_output.status.success() {
        // Check if the failure is due to missing git config
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        if stderr.contains("user.email") || stderr.contains("user.name") {
            return Err(CliError::BuildFailed {
                output: "Git commit failed: Please configure git with 'git config --global user.name \"Your Name\"' and 'git config --global user.email \"your.email@example.com\"'".to_string(),
            });
        } else {
            return Err(CliError::BuildFailed {
                output: format!("Failed to make initial commit: {}", stderr),
            });
        }
    }

    info!("Git repository initialized with initial commit");
    Ok(())
}
