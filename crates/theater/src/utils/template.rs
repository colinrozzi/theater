// Variable substitution using Handlebars

use handlebars::{Context, Handlebars, Helper, HelperResult, Output, RenderContext, RenderError};
use serde_json::Value;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Template rendering failed: {0}")]
    RenderError(#[from] RenderError),

    #[error("Template compilation failed: {0}")]
    CompilationError(String),
}

/// Substitute variables in TOML content using Handlebars templating
pub fn substitute_variables(toml_content: &str, state: &Value) -> Result<String, TemplateError> {
    let mut handlebars = Handlebars::new();

    // Register a helper for default values: {{default server.port "8080"}}
    handlebars.register_helper("default", Box::new(default_helper));

    // Compile and render the template
    let result = handlebars.render_template(toml_content, state)?;

    Ok(result)
}

/// Helper function to provide default values
/// Usage: {{default variable_path "default_value"}}
fn default_helper(
    h: &Helper,
    _: &Handlebars,
    _: &Context,
    _: &mut RenderContext,
    out: &mut dyn Output,
) -> HelperResult {
    // Get the first parameter (the variable we're trying to access)
    let value = h.param(0);

    // Get the default value (second parameter)
    let default = h.param(1).and_then(|v| v.value().as_str()).unwrap_or("");

    match value {
        Some(param) => {
            // If the value exists and isn't null/empty, use it
            match param.value() {
                Value::Null => out.write(default)?,
                Value::String(s) if s.is_empty() => out.write(default)?,
                other => out.write(&other.to_string())?,
            }
        }
        None => {
            // If the parameter doesn't exist, use default
            out.write(default)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_variable_substitution() {
        let toml = r#"name = "{{app.name}}""#;
        let state = json!({
            "app": {
                "name": "test-app"
            }
        });

        let result = substitute_variables(toml, &state).unwrap();
        assert_eq!(result, r#"name = "test-app""#);
    }

    #[test]
    fn test_nested_object_access() {
        let toml = r#"path = "{{workspace.directories.data}}""#;
        let state = json!({
            "workspace": {
                "directories": {
                    "data": "/var/data"
                }
            }
        });

        let result = substitute_variables(toml, &state).unwrap();
        assert_eq!(result, r#"path = "/var/data""#);
    }

    #[test]
    fn test_array_indexing() {
        let toml = r#"host = "{{servers.[0].hostname}}""#;
        let state = json!({
            "servers": [
                {"hostname": "server1.example.com"},
                {"hostname": "server2.example.com"}
            ]
        });

        let result = substitute_variables(toml, &state).unwrap();
        assert_eq!(result, r#"host = "server1.example.com""#);
    }

    #[test]
    fn test_boolean_and_number_substitution() {
        let toml = r#"
enabled = {{feature.enabled}}
count = {{config.max_items}}
"#;
        let state = json!({
            "feature": {"enabled": true},
            "config": {"max_items": 42}
        });

        let result = substitute_variables(toml, &state).unwrap();
        assert!(result.contains("enabled = true"));
        assert!(result.contains("count = 42"));
    }

    #[test]
    fn test_default_helper() {
        let toml = r#"port = {{default server.port "8080"}}"#;
        let state = json!({});

        let result = substitute_variables(toml, &state).unwrap();
        assert_eq!(result, r#"port = 8080"#);
    }

    #[test]
    fn test_default_helper_with_existing_value() {
        let toml = r#"port = {{default server.port "8080"}}"#;
        let state = json!({
            "server": {
                "port": 9000
            }
        });

        let result = substitute_variables(toml, &state).unwrap();
        assert_eq!(result, r#"port = 9000"#);
    }

    #[test]
    fn test_multiple_variables_same_line() {
        let toml = r#"url = "{{protocol}}://{{host}}:{{port}}""#;
        let state = json!({
            "protocol": "https",
            "host": "api.example.com",
            "port": 443
        });

        let result = substitute_variables(toml, &state).unwrap();
        assert_eq!(result, r#"url = "https://api.example.com:443""#);
    }

    #[test]
    fn test_complete_manifest_example() {
        let toml = r#"
name = "{{app.name}}"
version = "{{app.version}}"
package = "{{build.package_path}}"
description = "Processor for {{workspace.name}}"
save_chain = {{logging.save_events}}

[[handler]]
type = "filesystem"
path = "{{workspace.data_dir}}"
new_dir = {{filesystem.create_dirs}}

[[handler]]
type = "http-client"
base_url = "{{default api.endpoint "https://api.default.com"}}"
timeout = {{default api.timeout_ms "5000"}}
"#;

        let state = json!({
            "app": {
                "name": "dynamic-processor",
                "version": "0.1.0"
            },
            "build": {
                "package_path": "./dist/processor.wasm"
            },
            "workspace": {
                "name": "production-workspace",
                "data_dir": "/var/data/workspace-prod"
            },
            "api": {
                "endpoint": "https://prod-api.example.com/v1",
                "timeout_ms": 10000
            },
            "logging": {
                "save_events": true
            },
            "filesystem": {
                "create_dirs": true
            }
        });

        let result = substitute_variables(toml, &state).unwrap();

        println!("Rendered result:\n{}", result);

        // Verify key substitutions
        assert!(result.contains(r#"name = "dynamic-processor""#));
        assert!(result.contains(r#"version = "0.1.0""#));
        assert!(result.contains(r#"package = "./dist/processor.wasm""#));
        assert!(result.contains(r#"description = "Processor for production-workspace""#));
        assert!(result.contains("save_chain = true"));
        assert!(result.contains(r#"path = "/var/data/workspace-prod""#));
        assert!(result.contains("new_dir = true"));
        assert!(result.contains(r#"base_url = ""https://prod-api.example.com/v1"""#));
        assert!(result.contains("timeout = 10000"));
    }

    #[test]
    fn test_missing_variable_renders_empty() {
        let toml = r#"value = "{{missing.variable}}""#;
        let state = json!({});

        let result = substitute_variables(toml, &state).unwrap();
        // Handlebars renders missing variables as empty strings
        assert_eq!(result, r#"value = """#);
    }
}
