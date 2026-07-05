//! TLS context management for the TCP handler.
//!
//! This module provides configuration loading and TLS context creation from
//! manifest configuration.

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use theater::config::actor_manifest::TcpHandlerConfig;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{debug, info, warn};

/// Errors that can occur during TLS setup.
#[derive(Debug)]
pub enum TlsError {
    /// Failed to read certificate file
    CertificateRead(String, std::io::Error),
    /// Failed to parse certificate
    CertificateParse(String),
    /// Failed to read private key file
    KeyRead(String, std::io::Error),
    /// Failed to parse private key
    KeyParse(String),
    /// No valid private key found in file
    NoKey(String),
    /// Failed to build TLS configuration
    ConfigBuild(String),
}

impl std::fmt::Display for TlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TlsError::CertificateRead(path, err) => {
                write!(f, "Failed to read certificate file '{}': {}", path, err)
            }
            TlsError::CertificateParse(path) => {
                write!(f, "Failed to parse certificates from '{}'", path)
            }
            TlsError::KeyRead(path, err) => {
                write!(f, "Failed to read private key file '{}': {}", path, err)
            }
            TlsError::KeyParse(path) => {
                write!(f, "Failed to parse private key from '{}'", path)
            }
            TlsError::NoKey(path) => {
                write!(f, "No valid private key found in '{}'", path)
            }
            TlsError::ConfigBuild(msg) => {
                write!(f, "Failed to build TLS config: {}", msg)
            }
        }
    }
}

impl std::error::Error for TlsError {}

/// TLS context containing pre-configured client and server TLS settings.
///
/// This struct holds the rustls configurations needed for establishing TLS
/// connections. It's created once from the handler config and shared across
/// all connections.
pub struct TlsContext {
    /// TLS connector for outbound client connections
    pub client_connector: Option<TlsConnector>,
    /// TLS acceptor for inbound server connections
    pub server_acceptor: Option<TlsAcceptor>,
    /// Whether to TLS-handshake automatically on connect(). False means the
    /// connector is built but used only by explicit upgrade-to-tls-client
    /// calls (STARTTLS-style protocols).
    pub client_auto_handshake: bool,
}

impl TlsContext {
    /// Create a new TLS context from handler configuration.
    ///
    /// This reads certificate and key files and builds the rustls configurations.
    /// Returns `Ok(None)` if no TLS is configured.
    pub fn from_config(config: &TcpHandlerConfig) -> Result<Option<Self>, TlsError> {
        let client_connector = if let Some(ref client_tls) = config.client_tls {
            if client_tls.enabled {
                info!("Building TLS client configuration");
                Some(build_client_connector(client_tls)?)
            } else {
                debug!("Client TLS config present but not enabled");
                None
            }
        } else {
            None
        };

        let server_acceptor = if let Some(ref server_tls) = config.server_tls {
            if server_tls.enabled {
                info!("Building TLS server configuration");
                Some(build_server_acceptor(server_tls)?)
            } else {
                debug!("Server TLS config present but not enabled");
                None
            }
        } else {
            None
        };

        let client_auto_handshake = config
            .client_tls
            .as_ref()
            .map(|c| c.auto_handshake)
            .unwrap_or(true);

        if client_connector.is_none() && server_acceptor.is_none() {
            Ok(None)
        } else {
            Ok(Some(TlsContext {
                client_connector,
                server_acceptor,
                client_auto_handshake,
            }))
        }
    }
}

/// Build a TLS connector for client connections.
fn build_client_connector(
    config: &theater::config::actor_manifest::ClientTlsConfig,
) -> Result<TlsConnector, TlsError> {
    let mut root_store = RootCertStore::empty();

    // Add custom CA certificate if provided
    if let Some(ref ca_path) = config.ca_cert {
        info!("Loading custom CA certificate from: {:?}", ca_path);
        let certs = load_certificates(ca_path)?;
        for cert in certs {
            root_store
                .add(cert)
                .map_err(|e| TlsError::ConfigBuild(format!("Failed to add CA cert: {}", e)))?;
        }
        info!("Added {} custom CA certificates", root_store.len());
    } else {
        // Use Mozilla's root certificates
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        info!(
            "Using {} Mozilla root certificates",
            webpki_roots::TLS_SERVER_ROOTS.len()
        );
    }

    let client_config = if config.skip_verify {
        warn!("TLS certificate verification DISABLED - this is insecure!");
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth()
    } else {
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    };

    Ok(TlsConnector::from(Arc::new(client_config)))
}

/// Build a TLS acceptor for server connections.
fn build_server_acceptor(
    config: &theater::config::actor_manifest::ServerTlsConfig,
) -> Result<TlsAcceptor, TlsError> {
    info!("Loading server certificate from: {:?}", config.cert);
    let certs = load_certificates(&config.cert)?;
    info!("Loaded {} server certificates", certs.len());

    info!("Loading server private key from: {:?}", config.key);
    let key = load_private_key(&config.key)?;

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::ConfigBuild(format!("Failed to build server config: {}", e)))?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

/// Load PEM-encoded certificates from a file.
fn load_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let path_str = path.display().to_string();
    let file = File::open(path).map_err(|e| TlsError::CertificateRead(path_str.clone(), e))?;
    let mut reader = BufReader::new(file);

    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TlsError::CertificateParse(path_str.clone()))?;

    if certs.is_empty() {
        return Err(TlsError::CertificateParse(path_str));
    }

    Ok(certs)
}

/// Load a PEM-encoded private key from a file.
fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    let path_str = path.display().to_string();
    let file = File::open(path).map_err(|e| TlsError::KeyRead(path_str.clone(), e))?;
    let mut reader = BufReader::new(file);

    // Try to read any type of private key (RSA, PKCS8, EC)
    loop {
        match rustls_pemfile::read_one(&mut reader) {
            Ok(Some(rustls_pemfile::Item::Pkcs1Key(key))) => {
                return Ok(PrivateKeyDer::Pkcs1(key));
            }
            Ok(Some(rustls_pemfile::Item::Pkcs8Key(key))) => {
                return Ok(PrivateKeyDer::Pkcs8(key));
            }
            Ok(Some(rustls_pemfile::Item::Sec1Key(key))) => {
                return Ok(PrivateKeyDer::Sec1(key));
            }
            Ok(Some(_)) => {
                // Skip other items (like certificates)
                continue;
            }
            Ok(None) => {
                // End of file
                return Err(TlsError::NoKey(path_str));
            }
            Err(_) => {
                return Err(TlsError::KeyParse(path_str));
            }
        }
    }
}

/// Parse a server name from a string for TLS SNI.
///
/// This is used to set the SNI (Server Name Indication) extension when
/// establishing TLS connections.
pub fn parse_server_name(name: &str) -> Result<ServerName<'static>, TlsError> {
    // Try to parse as DNS name first
    ServerName::try_from(name.to_string())
        .map_err(|_| TlsError::ConfigBuild(format!("Invalid server name: {}", name)))
}

/// A certificate verifier that accepts any certificate (DANGEROUS!).
///
/// This is only used when `skip_verify` is enabled in the client config,
/// which should only be used for development/testing.
#[derive(Debug)]
pub(crate) struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theater::config::actor_manifest::TcpHandlerConfig;

    #[test]
    fn test_tls_context_no_config() {
        let config = TcpHandlerConfig::default();
        let ctx = TlsContext::from_config(&config).unwrap();
        assert!(ctx.is_none());
    }

    #[test]
    fn test_tls_context_disabled() {
        let config = TcpHandlerConfig {
            client_tls: Some(theater::config::actor_manifest::ClientTlsConfig {
                enabled: false,
                ca_cert: None,
                skip_verify: false,
                auto_handshake: true,
            }),
            server_tls: None,
            ..Default::default()
        };
        let ctx = TlsContext::from_config(&config).unwrap();
        assert!(ctx.is_none());
    }

    #[test]
    fn test_tls_error_display() {
        let err = TlsError::NoKey("/path/to/key.pem".to_string());
        assert!(err.to_string().contains("/path/to/key.pem"));
    }
}
