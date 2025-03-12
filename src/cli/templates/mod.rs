use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Template metadata
#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub files: HashMap<&'static str, &'static str>,
}

/// Available templates for creating new actors
pub fn available_templates() -> HashMap<String, Template> {
    let mut templates = HashMap::new();

    // Basic actor template
    templates.insert(
        "basic".to_string(),
        Template {
            name: "basic".to_string(),
            description: "A simple Theater actor with basic functionality".to_string(),
            files: basic_template_files(),
        },
    );

    // HTTP actor template
    templates.insert(
        "http".to_string(),
        Template {
            name: "http".to_string(),
            description: "An HTTP server actor with REST API and WebSocket support".to_string(),
            files: http_template_files(),
        },
    );

    templates
}

/// Basic actor template files
fn basic_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();
    
    // Add Cargo.toml
    files.insert(
        "Cargo.toml",
        include_str!("basic/Cargo.toml"),
    );
    
    // Add manifest.toml
    files.insert(
        "manifest.toml",
        include_str!("basic/manifest.toml"),
    );
    
    // Add src/lib.rs
    files.insert(
        "src/lib.rs",
        include_str!("basic/lib.rs"),
    );
    
    // Add README.md
    files.insert(
        "README.md",
        include_str!("basic/README.md"),
    );
    
    files
}

/// HTTP actor template files
fn http_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();
    
    // Add Cargo.toml
    files.insert(
        "Cargo.toml",
        include_str!("http/Cargo.toml"),
    );
    
    // Add manifest.toml
    files.insert(
        "manifest.toml",
        include_str!("http/manifest.toml"),
    );
    
    // Add src/lib.rs
    files.insert(
        "src/lib.rs",
        include_str!("http/lib.rs"),
    );
    
    // Add README.md
    files.insert(
        "README.md",
        include_str!("http/README.md"),
    );
    
    files
}

/// Create a new project from a template
pub fn create_project(template_name: &str, project_name: &str, output_dir: &Path) -> Result<()> {
    let templates = available_templates();
    let template = templates.get(template_name).ok_or_else(|| {
        anyhow!("Template '{}' not found. Available templates: {:?}", 
            template_name, 
            templates.keys().collect::<Vec<_>>())
    })?;
    
    info!("Creating new project '{}' using template '{}'", project_name, template_name);
    
    // Create the project directory
    let project_dir = output_dir.join(project_name);
    if project_dir.exists() {
        return Err(anyhow!("Directory already exists: {}", project_dir.display()));
    }
    
    fs::create_dir_all(&project_dir)?;
    debug!("Created project directory: {}", project_dir.display());
    
    // Create the src directory
    fs::create_dir_all(project_dir.join("src"))?;
    debug!("Created src directory: {}", project_dir.join("src").display());
    
    // Write all template files
    for (file_path, content) in &template.files {
        let dest_path = project_dir.join(file_path);
        
        // Create parent directory if needed
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Replace template variables
        let content = content
            .replace("{{project_name}}", project_name)
            .replace("{{project_name_snake}}", &project_name.replace('-', "_"));
        
        // Write the file
        fs::write(&dest_path, content)?;
        debug!("Created file: {}", dest_path.display());
    }
    
    info!("Successfully created project at {}", project_dir.display());
    
    Ok(())
}
