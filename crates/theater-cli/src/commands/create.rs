use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::debug;

use crate::{CommandContext, error::CliError, output::formatters::ProjectCreated, templates};

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
            "Project names must only contain alphanumeric characters, hyphens, and underscores"
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
    let templates_list = templates::available_templates();

    // Check if the template exists
    if !templates_list.contains_key(&args.template) {
        let available_templates: Vec<String> = templates_list.keys().cloned().collect();
        return Err(CliError::template_not_found(&args.template, available_templates));
    }

    // Create the project
    let project_path = output_dir.join(&args.name);
    templates::create_project(&args.template, &args.name, &output_dir)
        .map_err(|e| CliError::file_operation_failed("create project", project_path.display().to_string(), 
            std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    // Create success result and output
    let result = ProjectCreated {
        name: args.name.clone(),
        template: args.template.clone(),
        path: project_path,
        build_instructions: vec![
            format!("cd {}", args.name),
            "cargo build --target wasm32-unknown-unknown --release".to_string(),
            "theater start manifest.toml".to_string(),
        ],
    };

    ctx.output.output(&result, None)?;
    Ok(())
}

/// Legacy wrapper for backward compatibility
pub fn execute(args: &CreateArgs, verbose: bool, json: bool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let output = crate::output::OutputManager::new(config.output.clone());
        let ctx = crate::CommandContext {
            config,
            output,
            verbose,
            json,
        };
        execute_async(args, &ctx).await.map_err(|e| anyhow::Error::from(e))
    })
}

fn is_valid_project_name(name: &str) -> bool {
    // Check that the name only contains alphanumeric characters, hyphens, and underscores
    !name.is_empty() && name.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}
