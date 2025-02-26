#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_registry_initialization() {
        // Create a temporary directory for the registry
        let temp_dir = tempdir().unwrap();
        let registry_path = temp_dir.path();
        
        // Initialize registry
        let result = init_registry(registry_path);
        assert!(result.is_ok());
        
        // Verify structure
        assert!(registry_path.join("config.toml").exists());
        assert!(registry_path.join("index.toml").exists());
        assert!(registry_path.join("components").exists());
        assert!(registry_path.join("manifests").exists());
        assert!(registry_path.join("cache").exists());
    }
    
    #[test]
    fn test_registry_get_path() {
        // Test standard locations
        let path = get_registry_path();
        
        // This test is hard to make precise since it depends on the environment
        // We'll just check that it returns something reasonable or None
        if let Some(p) = path {
            assert!(p.ends_with("registry") || p.ends_with(".theater/registry"));
        }
    }
    
    #[test]
    fn test_registry_path_with_env_var() {
        // Set the environment variable
        std::env::set_var("THEATER_REGISTRY", "/tmp/test-registry");
        
        // Now it should pick up the env var path
        let path = get_registry_path();
        
        // Clean up
        std::env::remove_var("THEATER_REGISTRY");
        
        // Verify we don't get a path since the directory doesn't exist
        assert!(path.is_none());
    }
}
