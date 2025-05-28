use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::path::PathBuf;
use tracing::debug;

use crate::templates;

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

pub fn execute(args: &CreateArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Creating new actor project: {}", args.name);
    debug!("Using template: {}", args.template);

    // Check if the name is valid
    if !is_valid_project_name(&args.name) {
        return Err(anyhow!("Invalid project name: {}. Project names must only contain alphanumeric characters, hyphens, and underscores.", args.name));
    }

    // Get the output directory
    let output_dir = match &args.output_dir {
        Some(dir) => dir.clone(),
        None => std::env::current_dir()?,
    };

    debug!("Output directory: {}", output_dir.display());

    // Get available templates
    let templates_list = templates::available_templates();

    // Check if the template exists
    if !templates_list.contains_key(&args.template) {
        let available_templates = templates_list
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(anyhow!(
            "Template '{}' not found. Available templates: {}",
            args.template,
            available_templates
        ));
    }

    // Create the project
    templates::create_project(&args.template, &args.name, &output_dir)?;

    if !json {
        println!(
            "{} Created new actor project: {}",
            style("✓").green().bold(),
            style(&args.name).cyan()
        );

        println!("\nTo build and run your new actor:");
        println!("  cd {}", args.name);
        println!("  cargo build --target wasm32-unknown-unknown --release");
        println!("  theater start manifest.toml");
    } else {
        let output = serde_json::json!({
            "success": true,
            "project_name": args.name,
            "template": args.template,
            "path": output_dir.join(&args.name).display().to_string()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

fn is_valid_project_name(name: &str) -> bool {
    // Check that the name only contains alphanumeric characters, hyphens, and underscores
    name.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}
