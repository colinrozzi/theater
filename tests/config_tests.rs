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
