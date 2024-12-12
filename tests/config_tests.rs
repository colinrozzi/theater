use std::io::Write;
use tempfile::NamedTempFile;
use theater::config::ManifestConfig;

#[test]
fn test_manifest_loading() {
    let mut temp_file = NamedTempFile::new().unwrap();
    
    // Write test manifest content
    let manifest_content = r#"
name = "test-actor"
component_path = "test.wasm"

[interface]
implements = "ntwk:simple-actor/actor"
requires = []

[[handlers]]
type = "Http"
config = { port = 8080 }
"#;
    
    write!(temp_file, "{}", manifest_content).unwrap();
    
    // Load and verify manifest
    let config = ManifestConfig::from_file(temp_file.path()).unwrap();
    assert_eq!(config.name, "test-actor");
    assert_eq!(config.interface.implements, "ntwk:simple-actor/actor");
    assert!(config.interface.requires.is_empty());
}

#[test]
fn test_invalid_manifest() {
    let mut temp_file = NamedTempFile::new().unwrap();
    
    // Write invalid TOML content
    let invalid_content = r#"
name = "test-actor"
[invalid toml
"#;
    
    write!(temp_file, "{}", invalid_content).unwrap();
    
    // Verify loading fails
    assert!(ManifestConfig::from_file(temp_file.path()).is_err());
}

#[test]
fn test_interface_checking() {
    let mut temp_file = NamedTempFile::new().unwrap();
    
    // Write manifest with HTTP interface
    let manifest_content = r#"
name = "http-actor"
component_path = "test.wasm"

[interface]
implements = "ntwk:simple-http-actor/http-actor"
requires = []

[[handlers]]
type = "Http"
config = { port = 8080 }
"#;
    
    write!(temp_file, "{}", manifest_content).unwrap();
    
    let config = ManifestConfig::from_file(temp_file.path()).unwrap();
    
    // Test interface checks
    assert!(config.implements_interface("ntwk:simple-http-actor/http-actor"));
    assert!(!config.implements_interface("ntwk:simple-actor/actor"));
    assert_eq!(config.interface(), "ntwk:simple-http-actor/http-actor");
}
