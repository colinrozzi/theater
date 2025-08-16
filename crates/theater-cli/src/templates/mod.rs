use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use handlebars::Handlebars;
use serde::{Deserialize, Serialize};

/// Template metadata loaded from template.toml
#[derive(Debug, Clone, Deserialize)]
pub struct TemplateMetadata {
    pub template: TemplateInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
    pub files: HashMap<String, String>,
}

/// Template data for rendering
#[derive(Debug, Clone, Serialize)]
pub struct TemplateData {
    pub project_name: String,
    pub project_name_snake: String,
}

/// Get the path to the templates directory
fn templates_dir() -> PathBuf {
    // Get the path relative to the CLI crate root
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("templates")
}

/// Available templates for creating new actors
pub fn available_templates() -> Result<HashMap<String, TemplateInfo>, io::Error> {
    let mut templates = HashMap::new();
    let templates_path = templates_dir();
    
    if !templates_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Templates directory not found: {}", templates_path.display())
        ));
    }

    // Read all template directories
    for entry in fs::read_dir(&templates_path)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_dir() {
            let template_name = path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid template directory name"
                ))?;
            
            // Load template.toml
            let metadata_path = path.join("template.toml");
            if metadata_path.exists() {
                match load_template_metadata(&metadata_path) {
                    Ok(metadata) => {
                        debug!("Loaded template: {} - {}", template_name, metadata.template.description);
                        templates.insert(template_name.to_string(), metadata.template);
                    }
                    Err(e) => {
                        debug!("Failed to load template {}: {}", template_name, e);
                    }
                }
            } else {
                debug!("Template {} missing template.toml, skipping", template_name);
            }
        }
    }

    if templates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No valid templates found"
        ));
    }

    Ok(templates)
}

/// Load template metadata from template.toml
fn load_template_metadata(path: &Path) -> Result<TemplateMetadata, io::Error> {
    let content = fs::read_to_string(path)?;
    toml::from_str(&content).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid template.toml: {}", e)
        )
    })
}

/// Create a new actor project from a template
pub fn create_project(
    template_name: &str,
    project_name: &str,
    target_dir: &Path,
) -> Result<(), io::Error> {
    let templates = available_templates()?;
    let template = templates
        .get(template_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Template not found"))?;

    info!(
        "Creating new {} project '{}' in {}",
        template_name,
        project_name,
        target_dir.display()
    );

    // Create the target directory
    fs::create_dir_all(target_dir)?;

    // Setup Handlebars renderer
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    
    // Register default helper (this should match what's used in the main theater crate)
    handlebars.register_helper("default", Box::new(|h: &handlebars::Helper, _: &Handlebars, _: &handlebars::Context, _: &mut handlebars::RenderContext, out: &mut dyn handlebars::Output| -> handlebars::HelperResult {
        let value = h.param(0).and_then(|v| v.value().as_str());
        let default = h.param(1).and_then(|v| v.value().as_str()).unwrap_or("");
        
        let result = if let Some(val) = value {
            if val.is_empty() { default } else { val }
        } else {
            default
        };
        
        out.write(result)?;
        Ok(())
    }));

    // Prepare template data
    let template_data = TemplateData {
        project_name: project_name.to_string(),
        project_name_snake: project_name.replace('-', "_"),
    };

    // Get template directory
    let template_dir = templates_dir().join(template_name);
    
    // Create all template files
    for (target_path, template_file) in &template.files {
        let source_file_path = template_dir.join(template_file);
        let target_file_path = target_dir.join(target_path);

        // Create parent directories if they don't exist
        if let Some(parent) = target_file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Read template content
        let template_content = fs::read_to_string(&source_file_path)
            .map_err(|e| io::Error::new(
                io::ErrorKind::NotFound,
                format!("Template file not found: {} ({})", source_file_path.display(), e)
            ))?;

        // Render template with Handlebars
        let rendered_content = handlebars
            .render_template(&template_content, &template_data)
            .map_err(|e| io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Template rendering failed for {}: {}", template_file, e)
            ))?;

        debug!(
            "Creating file: {} ({} bytes)",
            target_file_path.display(),
            rendered_content.len()
        );

        // Write the rendered file
        fs::write(&target_file_path, rendered_content)?;
    }

    info!("Project '{}' created successfully!", project_name);
    info!("Note: You may need to run 'wkg wit fetch' to fetch WIT dependencies");
    
    Ok(())
}

/// List all available templates
pub fn list_templates() -> Result<(), io::Error> {
    let templates = available_templates()?;
    
    println!("Available templates:");
    for (name, template) in templates {
        println!("  {}: {}", name, template.description);
    }
    
    Ok(())
}
