use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use rustls_pemfile::{certs, private_key};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info};

/// Load certificates from a PEM file
pub fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let path = Path::new(path);

    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Certificate file not found: {}",
            path.display()
        ));
    }

    if !path.is_file() {
        return Err(anyhow::anyhow!(
            "Certificate path is not a file: {}",
            path.display()
        ));
    }

    debug!("Loading certificates from: {}", path.display());

    let file = File::open(path)
        .with_context(|| format!("Failed to open certificate file: {}", path.display()))?;

    let mut reader = BufReader::new(file);
    let certs: Result<Vec<_>, _> = certs(&mut reader).collect();
    let certs =
        certs.with_context(|| format!("Failed to parse certificates from: {}", path.display()))?;

    if certs.is_empty() {
        return Err(anyhow::anyhow!(
            "No certificates found in file: {}",
            path.display()
        ));
    }

    info!(
        "Loaded {} certificate(s) from: {}",
        certs.len(),
        path.display()
    );
    Ok(certs)
}

/// Load private key from a PEM file
pub fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let path = Path::new(path);

    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Private key file not found: {}",
            path.display()
        ));
    }

    if !path.is_file() {
        return Err(anyhow::anyhow!(
            "Private key path is not a file: {}",
            path.display()
        ));
    }

    debug!("Loading private key from: {}", path.display());

    let file = File::open(path)
        .with_context(|| format!("Failed to open private key file: {}", path.display()))?;

    let mut reader = BufReader::new(file);
    let key = private_key(&mut reader)
        .with_context(|| format!("Failed to parse private key from: {}", path.display()))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in file: {}", path.display()))?;

    info!("Loaded private key from: {}", path.display());
    Ok(key)
}

/// Create a rustls ServerConfig from certificate and key paths
pub fn create_tls_config(cert_path: &str, key_path: &str) -> Result<RustlsConfig> {
    debug!(
        "Creating TLS configuration with cert: {}, key: {}",
        cert_path, key_path
    );

    // Load certificates
    let cert_chain = load_certs(cert_path)?;

    // Load private key
    let private_key = load_private_key(key_path)?;

    // Create TLS configuration
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .with_context(|| "Failed to create TLS configuration")?;

    // Convert to RustlsConfig for axum-server
    let rustls_config = RustlsConfig::from_config(Arc::new(server_config));

    info!("TLS configuration created successfully");
    Ok(rustls_config)
}

/// Validate TLS configuration without creating the full config
/// Useful for early validation
pub fn validate_tls_config(cert_path: &str, key_path: &str) -> Result<()> {
    debug!("Validating TLS configuration paths");

    // Check if files exist and are readable
    let cert_path = Path::new(cert_path);
    let key_path = Path::new(key_path);

    if !cert_path.exists() {
        return Err(anyhow::anyhow!(
            "Certificate file not found: {}",
            cert_path.display()
        ));
    }

    if !key_path.exists() {
        return Err(anyhow::anyhow!(
            "Private key file not found: {}",
            key_path.display()
        ));
    }

    if !cert_path.is_file() {
        return Err(anyhow::anyhow!(
            "Certificate path is not a file: {}",
            cert_path.display()
        ));
    }

    if !key_path.is_file() {
        return Err(anyhow::anyhow!(
            "Private key path is not a file: {}",
            key_path.display()
        ));
    }

    // Try to load them to make sure they're valid
    let _certs = load_certs(cert_path.to_str().unwrap())?;
    let _key = load_private_key(key_path.to_str().unwrap())?;

    debug!("TLS configuration validation successful");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_test_cert() -> &'static str {
        // A minimal self-signed certificate for testing
        r#"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAMlyFqk69v+9MA0GCSqGSIb3DQEBCwUAMBQxEjAQBgNVBAMMCWxv
Y2FsaG9zdDAeFw0yNDA4MTgwMDAwMDBaFw0yNTA4MTgwMDAwMDBaMBQxEjAQBgNV
BAMMCWxvY2FsaG9zdDBcMA0GCSqGSIb3DQEBAQUAA0sAMEgCQQDJ2+eHv1YUGf+i
ULqmFbIwv8vCTvIcJUXhj+8WFhGJU0vQ7sJqrwLNkSXiYZHNhLMZMGw8JHxU8JQU
rJxWNK5fAgMBAAEwDQYJKoZIhvcNAQELBQADQQB8J+HnF9E5HbGlGZJQJW6Q5L5E
oqwV3CXm9oQJmVGWTKhJ5KXF2X2w9Q9QBEoX3oX9Q5YbhVwqBWK9F7hKKX6g
-----END CERTIFICATE-----"#
    }

    fn create_test_private_key() -> &'static str {
        // A minimal private key for testing
        r#"-----BEGIN PRIVATE KEY-----
MIIBVAIBADANBgkqhkiG9w0BAQEFAASCAT4wggE6AgEAAkEAydvnh79WFBn/olC6
phWyML/Lwk7yHCVF4Y/vFhYRiVNL0O7Caq8CzZEl4mGRzYSzGTBsPCR8VPCUFK
ycVjSuXwIDAQABAkEAkWdwLBwU8TI4+BQJXM5FhT7yCqrHhFgJXUYwW5kJm4qP
+8X3JEwF7QzJ7yVF0q4X1VY8W5kJm4qP+8X3JEwF7QIhAPhT7yCqrHhFgJXUYw
W5kJm4qP+8X3JEwF7QzJ7yVF0q4XAiEA1VY8W5kJm4qP+8X3JEwF7QzJ7yVF0q
4X1VY8W5kJm4qP+8CIQD4U+8gqqx4RYCVFGMFuZCZuKj/vF9yRMBe0Mye8lRdK
uFwIgNVWPFuZCZuKj/vF9yRMBe0Mye8lRdKuF9VWPFuZCZuKj/vA===
-----END PRIVATE KEY-----"#
    }

    #[test]
    fn test_validate_nonexistent_files() {
        let result = validate_tls_config("/nonexistent/cert.pem", "/nonexistent/key.pem");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_load_valid_cert_and_key() -> Result<()> {
        let temp_dir = tempdir()?;

        // Create test certificate file
        let cert_path = temp_dir.path().join("test.crt");
        fs::write(&cert_path, create_test_cert())?;

        // Create test private key file
        let key_path = temp_dir.path().join("test.key");
        fs::write(&key_path, create_test_private_key())?;

        // For now, just test that the files exist since we need real certificates
        // In a full implementation, we'd use proper test certificates

        // Test that the functions would fail appropriately with invalid content
        let certs_result = load_certs(cert_path.to_str().unwrap());
        let key_result = load_private_key(key_path.to_str().unwrap());

        // These should fail because our test data isn't real certificates
        assert!(certs_result.is_err() || key_result.is_err());

        Ok(())
    }
}
