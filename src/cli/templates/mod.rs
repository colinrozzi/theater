use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

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
    files.insert("Cargo.toml", include_str!("basic/Cargo.toml"));

    // Add manifest.toml
    files.insert("manifest.toml", include_str!("basic/manifest.toml"));

    // Add src/lib.rs
    files.insert("src/lib.rs", include_str!("basic/lib.rs"));

    // Add README.md
    files.insert("README.md", include_str!("basic/README.md"));

    files
}

/// HTTP actor template files
fn http_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();

    // Add Cargo.toml
    files.insert("Cargo.toml", include_str!("http/Cargo.toml"));

    // Add manifest.toml
    files.insert("manifest.toml", include_str!("http/manifest.toml"));

    // Add src/lib.rs
    files.insert("src/lib.rs", include_str!("http/lib.rs"));

    // Add README.md
    files.insert("README.md", include_str!("http/README.md"));

    files
}

/// Get the path to the theater WIT files
fn get_theater_wit_dir() -> PathBuf {
    // Get the path to the theater/wit directory
    // We know that this code is running within the theater project,
    // and the theater project has a wit directory at its root

    // First try using CARGO_MANIFEST_DIR which points to the directory containing the Cargo.toml
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let from_manifest = manifest_dir.join("wit");

    if from_manifest.exists() && from_manifest.is_dir() {
        return from_manifest;
    }

    // Fallback to current directory and search upwards
    let mut current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Try to find the wit directory by walking up the directory tree
    loop {
        let wit_dir = current_dir.join("wit");
        if wit_dir.exists() && wit_dir.is_dir() {
            return wit_dir;
        }

        // Go up one directory
        if !current_dir.pop() {
            break;
        }
    }

    // Last resort - hardcode the path based on the expected project structure
    debug!("Could not find wit directory, using hardcoded path");
    PathBuf::from("/Users/colinrozzi/work/theater/wit")
}

/// Copy a file from source to destination
fn copy_file(source: &Path, dest: &Path) -> io::Result<()> {
    debug!(
        "Copying file from {} to {}",
        source.display(),
        dest.display()
    );
    fs::copy(source, dest)?;
    Ok(())
}

/// Copy WIT files from theater/wit to the new project's wit directory
fn copy_wit_files(project_dir: &Path, template_name: &str) -> Result<()> {
    let theater_wit_dir = get_theater_wit_dir();
    debug!("Theater WIT directory: {}", theater_wit_dir.display());

    // Create the wit directory in the project
    let project_wit_dir = project_dir.join("wit");
    fs::create_dir_all(&project_wit_dir)?;
    debug!("Created wit directory: {}", project_wit_dir.display());

    // Copy the world.wit file from the template
    // The template directory should be in src/cli/templates relative to manifest dir
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let template_dir = manifest_dir
        .join("src")
        .join("cli")
        .join("templates")
        .join(template_name);
    let world_wit_path = template_dir.join("world.wit");

    debug!("Looking for world.wit at: {}", world_wit_path.display());

    if world_wit_path.exists() {
        copy_file(&world_wit_path, &project_wit_dir.join("world.wit"))?;
        debug!("Copied world.wit from template");
    } else {
        warn!(
            "world.wit not found in template: {}",
            world_wit_path.display()
        );
        // Try a different location - directly in the templates directory
        let alt_path = Path::new(file!())
            .parent()
            .unwrap_or(Path::new("."))
            .join(template_name)
            .join("world.wit");
        debug!("Trying alternative path: {}", alt_path.display());

        if alt_path.exists() {
            copy_file(&alt_path, &project_wit_dir.join("world.wit"))?;
            debug!("Copied world.wit from alternative path");
        } else {
            warn!(
                "world.wit not found in alternative path: {}",
                alt_path.display()
            );
        }
    }

    // Copy all .wit files from theater/wit to the project's wit directory
    if theater_wit_dir.exists() {
        match fs::read_dir(&theater_wit_dir) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file() && path.extension().map_or(false, |ext| ext == "wit") {
                            let file_name = path.file_name().unwrap();
                            let dest_path = project_wit_dir.join(file_name);

                            // Skip if already copied (e.g., we've already copied the world.wit)
                            if !dest_path.exists() {
                                copy_file(&path, &dest_path)?;
                                debug!("Copied {} to wit directory", file_name.to_string_lossy());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read theater wit directory: {}", e);
            }
        }
    } else {
        warn!(
            "Theater wit directory not found: {}",
            theater_wit_dir.display()
        );
    }

    Ok(())
}

/// Create a new project from a template
pub fn create_project(template_name: &str, project_name: &str, output_dir: &Path) -> Result<()> {
    // Get absolute path of the output directory
    let abs_output_dir = if output_dir.is_absolute() {
        output_dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(output_dir)
    };
    debug!("Absolute output directory: {}", abs_output_dir.display());
    let templates = available_templates();
    let template = templates.get(template_name).ok_or_else(|| {
        anyhow!(
            "Template '{}' not found. Available templates: {:?}",
            template_name,
            templates.keys().collect::<Vec<_>>()
        )
    })?;

    info!(
        "Creating new project '{}' using template '{}'",
        project_name, template_name
    );

    // Create the project directory
    let project_dir = abs_output_dir.join(project_name);
    if project_dir.exists() {
        return Err(anyhow!(
            "Directory already exists: {}",
            project_dir.display()
        ));
    }

    fs::create_dir_all(&project_dir)?;
    debug!("Created project directory: {}", project_dir.display());

    // Create the src directory
    fs::create_dir_all(project_dir.join("src"))?;
    debug!(
        "Created src directory: {}",
        project_dir.join("src").display()
    );

    // Write all template files
    for (file_path, content) in &template.files {
        let dest_path = project_dir.join(file_path);

        // Create parent directory if needed
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Replace template variables
        let mut content = content
            .replace("{{project_name}}", project_name)
            .replace("{{project_name_snake}}", &project_name.replace('-', "_"));

        // If this is the manifest.toml file, replace the component_path with absolute path
        if *file_path == "manifest.toml" {
            // Construct the absolute path to the WASM file
            let project_name_snake = project_name.replace('-', "_");
            let wasm_rel_path = format!(
                "target/wasm32-unknown-unknown/release/{}.wasm",
                project_name_snake
            );
            let wasm_abs_path = project_dir.join(&wasm_rel_path);

            // Replace the relative component_path with the absolute path
            // First find the line with component_path
            if let Some(start_pos) = content.find("component_path = ") {
                let end_pos = content[start_pos..]
                    .find('\n')
                    .map_or(content.len(), |pos| start_pos + pos);
                let original_line = &content[start_pos..end_pos];
                let new_line = format!("component_path = \"{}\"", wasm_abs_path.display());

                // Replace the specific line
                content = content.replace(original_line, &new_line);
            }

            debug!("Set absolute component_path: {}", wasm_abs_path.display());
        }

        // Write the file
        fs::write(&dest_path, content)?;
        debug!("Created file: {}", dest_path.display());
    }

    // Copy WIT files
    if let Err(e) = copy_wit_files(&project_dir, template_name) {
        warn!("Failed to copy WIT files: {}", e);
    }

    info!("Successfully created project at {}", project_dir.display());

    Ok(())
}
