use anyhow::{anyhow, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::debug;

use crate::cli::utils::formatting;

// Define the manifest structure (similar to what's used in the theater_server)
#[derive(Debug, Deserialize, Serialize)]
struct HandlerConfig {
    #[serde(rename = "type")]
    handler_type: String,
    #[serde(flatten)]
    config: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct InterfaceConfig {
    implements: String,
    requires: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ActorManifest {
    name: String,
    component_path: String,
    interface: Option<InterfaceConfig>,
    #[serde(default)]
    handlers: Vec<HandlerConfig>,
}

// Validation result for individual checks
#[derive(Debug)]
enum ValidationLevel {
    Error,
    Warning,
    Info,
}

// Result of a validation check
#[derive(Debug)]
struct ValidationResult {
    level: ValidationLevel,
    message: String,
    line: Option<usize>,
}

#[derive(Debug, Parser)]
pub struct ValidateArgs {
    /// Path to the manifest file
    #[arg(required = true)]
    pub manifest: PathBuf,

    /// Check that component file exists
    #[arg(short, long, default_value = "true")]
    pub check_paths: bool,

    /// Check interface compatibility
    #[arg(long)]
    pub check_interfaces: bool,
}

pub fn execute(args: &ValidateArgs, verbose: bool, json: bool) -> Result<()> {
    debug!("Validating manifest file: {}", args.manifest.display());

    // Check if the manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow!(
            "Manifest file not found: {}",
            args.manifest.display()
        ));
    }

    // Read and parse the manifest file
    let manifest_content = std::fs::read_to_string(&args.manifest)?;

    // Try to parse as TOML
    let manifest: Result<ActorManifest, _> = toml::from_str(&manifest_content);

    // Collect validation results
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut infos = Vec::new();

    match manifest {
        Ok(manifest) => {
            debug!("Successfully parsed manifest");

            // Validate the manifest content
            validate_manifest(
                &manifest,
                &args.manifest,
                args.check_paths,
                args.check_interfaces,
                &mut errors,
                &mut warnings,
                &mut infos,
            )?;
        }
        Err(e) => {
            // Add parse error
            errors.push(ValidationResult {
                level: ValidationLevel::Error,
                message: format!("Failed to parse manifest: {}", e),
                line: None, // In a more advanced implementation, we could extract line numbers from TOML parse errors
            });
        }
    }

    // JSON output
    if json {
        let output = json!({
            "manifest": args.manifest.display().to_string(),
            "valid": errors.is_empty(),
            "errors": errors.iter().map(|r| json!({
                "message": r.message,
                "line": r.line
            })).collect::<Vec<_>>(),
            "warnings": warnings.iter().map(|r| json!({
                "message": r.message,
                "line": r.line
            })).collect::<Vec<_>>(),
            "info": infos.iter().map(|r| json!({
                "message": r.message,
                "line": r.line
            })).collect::<Vec<_>>()
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Human-readable output
    println!("{}", formatting::format_section("MANIFEST VALIDATION"));
    println!("Manifest: {}\n", args.manifest.display());

    // Print summary
    if errors.is_empty() && warnings.is_empty() {
        println!("{}", formatting::format_success("Manifest is valid."));
    } else {
        if !errors.is_empty() {
            println!(
                "{}",
                formatting::format_error(&format!("Manifest has {} error(s).", errors.len()))
            );
        }

        if !warnings.is_empty() {
            println!(
                "{}",
                formatting::format_warning(&format!("Manifest has {} warning(s).", warnings.len()))
            );
        }
    }

    // Print errors
    if !errors.is_empty() {
        println!("\n{}", formatting::format_section("ERRORS"));
        for (i, error) in errors.iter().enumerate() {
            print_validation_result(i + 1, error);
        }
    }

    // Print warnings
    if !warnings.is_empty() {
        println!("\n{}", formatting::format_section("WARNINGS"));
        for (i, warning) in warnings.iter().enumerate() {
            print_validation_result(i + 1, warning);
        }
    }

    // Print info
    if !infos.is_empty() && verbose {
        println!("\n{}", formatting::format_section("INFORMATION"));
        for (i, info) in infos.iter().enumerate() {
            print_validation_result(i + 1, info);
        }
    }

    Ok(())
}

/// Validate the manifest content
fn validate_manifest(
    manifest: &ActorManifest,
    _manifest_path: &Path,
    check_paths: bool,
    check_interfaces: bool,
    errors: &mut Vec<ValidationResult>,
    warnings: &mut Vec<ValidationResult>,
    infos: &mut Vec<ValidationResult>,
) -> Result<()> {
    // 1. Validate name
    if manifest.name.is_empty() {
        errors.push(ValidationResult {
            level: ValidationLevel::Error,
            message: "Actor name cannot be empty".to_string(),
            line: None,
        });
    } else if !manifest
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        warnings.push(ValidationResult {
            level: ValidationLevel::Warning,
            message: format!("Actor name '{}' contains characters other than alphanumeric, hyphen, or underscore", manifest.name),
            line: None,
        });
    }

    // 2. Validate component path
    if manifest.component_path.is_empty() {
        errors.push(ValidationResult {
            level: ValidationLevel::Error,
            message: "Component path cannot be empty".to_string(),
            line: None,
        });
    } else if check_paths {
        let component_path = Path::new(&manifest.component_path);
        if !component_path.exists() {
            errors.push(ValidationResult {
                level: ValidationLevel::Error,
                message: format!("Component file not found: {}", manifest.component_path),
                line: None,
            });
        } else if !component_path
            .extension()
            .map_or(false, |ext| ext == "wasm")
        {
            warnings.push(ValidationResult {
                level: ValidationLevel::Warning,
                message: format!(
                    "Component file does not have .wasm extension: {}",
                    manifest.component_path
                ),
                line: None,
            });
        }
    }

    // 3. Validate interface
    if let Some(interface) = &manifest.interface {
        if interface.implements.is_empty() {
            errors.push(ValidationResult {
                level: ValidationLevel::Error,
                message: "Interface 'implements' cannot be empty".to_string(),
                line: None,
            });
        }

        // Check interface compatibility if requested
        if check_interfaces {
            // This would involve checking the actual WebAssembly component
            // against the specified interfaces, which is beyond the scope
            // of this basic validator
            infos.push(ValidationResult {
                level: ValidationLevel::Info,
                message: "Interface compatibility checking is not yet implemented".to_string(),
                line: None,
            });
        }
    } else {
        // No interface specified
        warnings.push(ValidationResult {
            level: ValidationLevel::Warning,
            message: "No interface specified in manifest".to_string(),
            line: None,
        });
    }

    // 4. Validate handlers
    for (i, handler) in manifest.handlers.iter().enumerate() {
        // Check handler type
        match handler.handler_type.as_str() {
            "message-server" | "http-server" | "supervisor" => {
                // These are known handler types
            }
            _ => {
                warnings.push(ValidationResult {
                    level: ValidationLevel::Warning,
                    message: format!("Unknown handler type: {}", handler.handler_type),
                    line: None,
                });
            }
        }

        // Check handler configuration
        match handler.handler_type.as_str() {
            "message-server" => {
                // Check for port configuration
                if !handler.config.get("port").is_some() {
                    warnings.push(ValidationResult {
                        level: ValidationLevel::Warning,
                        message: format!(
                            "Handler {} (message-server) is missing port configuration",
                            i
                        ),
                        line: None,
                    });
                }
            }
            "http-server" => {
                // Check for port configuration
                if !handler.config.get("port").is_some() {
                    warnings.push(ValidationResult {
                        level: ValidationLevel::Warning,
                        message: format!(
                            "Handler {} (http-server) is missing port configuration",
                            i
                        ),
                        line: None,
                    });
                }
            }
            _ => {} // Other handler types don't need specific validation
        }
    }

    Ok(())
}

/// Print a validation result
fn print_validation_result(index: usize, result: &ValidationResult) {
    let prefix = match result.level {
        ValidationLevel::Error => formatting::format_error(&format!("{}.", index)),
        ValidationLevel::Warning => formatting::format_warning(&format!("{}.", index)),
        ValidationLevel::Info => formatting::format_info(&format!("{}.", index)),
    };

    let location = if let Some(line) = result.line {
        format!(" (line {})", line)
    } else {
        String::new()
    };

    println!("{} {}{}", prefix, result.message, location);
}
