// Integration tests for manifest variable substitution

use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;
use serde_json::json;

use theater::config::actor_manifest::ManifestConfig;

#[tokio::test]
async fn test_manifest_loading_with_substitution() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create initial state file
    let init_state = json!({
        "app": {
            "name": "test-processor",
            "version": "1.0.0"
        },
        "build": {
            "package_path": "./processor.wasm"
        },
        "server": {
            "port": 8080
        },
        "features": {
            "debug": true
        }
    });
    
    let init_state_path = temp_dir.path().join("config.json");
    fs::write(&init_state_path, serde_json::to_string_pretty(&init_state).unwrap())
        .await
        .unwrap();
    
    // Create manifest with variables
    let manifest_content = format!(r#"
name = "${{app.name}}"
version = "${{app.version}}"
package = "${{build.package_path}}"
description = "Test processor for ${{app.name}}"
save_chain = ${{features.debug}}
init_state = "{}"

[[handler]]
type = "filesystem"
path = "/tmp/data"
new_dir = false

[[handler]]
type = "http-server"
port = ${{server.port}}
"#, init_state_path.to_string_lossy());
    
    // Load manifest with substitution
    let config = ManifestConfig::from_str_with_substitution(&manifest_content, None)
        .await
        .unwrap();
    
    // Verify substitutions worked
    assert_eq!(config.name, "test-processor");
    assert_eq!(config.version, "1.0.0");
    assert_eq!(config.package, "./processor.wasm");
    assert_eq!(config.description, Some("Test processor for test-processor".to_string()));
    assert_eq!(config.save_chain, Some(true));
    
    // Verify handlers
    assert_eq!(config.handlers.len(), 2);
}

#[tokio::test]
async fn test_manifest_with_override_state() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create initial state file
    let init_state = json!({
        "app": {
            "name": "base-app"
        },
        "server": {
            "port": 3000
        }
    });
    
    let init_state_path = temp_dir.path().join("config.json");
    fs::write(&init_state_path, serde_json::to_string_pretty(&init_state).unwrap())
        .await
        .unwrap();
    
    // Create manifest with variables
    let manifest_content = format!(r#"
name = "${{app.name}}"
package = "test.wasm"
version = "1.0.0"
init_state = "{}"

[[handler]]
type = "http-server"
port = ${{server.port}}
"#, init_state_path.to_string_lossy());
    
    // Override state that changes app name and adds new field
    let override_state = json!({
        "app": {
            "name": "overridden-app"
        },
        "server": {
            "port": 8080
        }
    });
    
    // Load manifest with substitution and override
    let config = ManifestConfig::from_str_with_substitution(&manifest_content, Some(&override_state))
        .await
        .unwrap();
    
    // Verify override took precedence
    assert_eq!(config.name, "overridden-app");
}

#[tokio::test]
async fn test_manifest_without_variables() {
    // Test that manifests without variables still work
    let manifest_content = r#"
name = "static-app"
version = "1.0.0"
package = "static.wasm"
description = "A static app"

[[handler]]
type = "filesystem"
path = "/static/path"
"#;
    
    // Load without substitution
    let config1 = ManifestConfig::from_str(manifest_content).unwrap();
    
    // Load with substitution but no state
    let config2 = ManifestConfig::from_str_with_substitution(manifest_content, None)
        .await
        .unwrap();
    
    // Both should be identical
    assert_eq!(config1.name, config2.name);
    assert_eq!(config1.version, config2.version);
    assert_eq!(config1.package, config2.package);
    assert_eq!(config1.description, config2.description);
    assert_eq!(config1.handlers.len(), config2.handlers.len());
}

#[tokio::test]
async fn test_manifest_missing_variable_error() {
    let manifest_content = r#"
name = "${missing.variable}"
version = "1.0.0"
package = "test.wasm"
"#;
    
    let result = ManifestConfig::from_str_with_substitution(manifest_content, None).await;
    
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Variable substitution failed"));
}

#[tokio::test]
async fn test_manifest_with_defaults() {
    let manifest_content = r#"
name = "${app.name:default-app}"
version = "1.0.0"
package = "test.wasm"
save_chain = ${logging.enabled:false}

[[handler]]
type = "http-server"  
port = ${server.port:3000}
"#;
    
    // Load with empty state - should use defaults
    let config = ManifestConfig::from_str_with_substitution(manifest_content, Some(&json!({})))
        .await
        .unwrap();
    
    assert_eq!(config.name, "default-app");
    assert_eq!(config.save_chain, Some(false));
}

#[tokio::test]
async fn test_backward_compatibility() {
    // Test that all existing manifest loading methods still work
    let manifest_content = r#"
name = "compat-test"
version = "1.0.0"
package = "test.wasm"

[[handler]]
type = "filesystem"
path = "/tmp"
"#;
    
    // Test from_str (original method)
    let config1 = ManifestConfig::from_str(manifest_content).unwrap();
    
    // Test from_str_with_substitution with no variables
    let config2 = ManifestConfig::from_str_with_substitution(manifest_content, None)
        .await
        .unwrap();
    
    // Should be identical
    assert_eq!(config1.name, config2.name);
    assert_eq!(config1.version, config2.version);
    assert_eq!(config1.package, config2.package);
}
