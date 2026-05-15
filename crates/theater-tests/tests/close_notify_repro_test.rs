//! Integration test driving the `close-notify-tls-repro` test-actor.
//!
//! Spins up theater (via the `target/debug/theater start` subprocess) on a
//! TLS listener, points it at a dummy TLS outbound the test owns, then
//! probes the actor with a strict rustls client. The actor is the
//! actor-per-connection (acceptor + handler) pattern from `test-actors/
//! close-notify-tls-repro/`, which mirrors the inbox `/send` close path:
//! inbound TLS, outbound TLS during handling, then tcp_send + tcp_close +
//! shutdown(None).
//!
//! Currently this test passes — the close path on the inbound returns clean
//! EOF (close_notify properly flushed). It exists as a regression test that
//! locks in the contract for the two-TLS-streams shape and as the local
//! scaffold for any future bisect when the prod `/send` repro is finally
//! pinned to a code path we can mirror locally.
//!
//! Ignored by default — needs the test-actor wasms built and an available
//! 127.0.0.1:18443 + :18444. Run with:
//!   cd test-actors/close-notify-tls-repro && cargo build --release --target wasm32-unknown-unknown
//!   cargo test -p theater-tests --test close_notify_repro_test -- --ignored --nocapture

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, ServerConfig, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector};

const INBOUND_PORT: u16 = 18443;
const OUTBOUND_PORT: u16 = 18444;
const REPRO_DIR: &str = "/tmp/close_notify_repro";

#[derive(Debug)]
struct AcceptAny;

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::ED25519,
        ]
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn ensure_wasm_built() -> (PathBuf, PathBuf) {
    let root = workspace_root();
    let actor_dir = root.join("test-actors/close-notify-tls-repro");
    let acceptor = actor_dir
        .join("target/wasm32-unknown-unknown/release/close_notify_tls_repro_acceptor.wasm");
    let handler =
        actor_dir.join("target/wasm32-unknown-unknown/release/close_notify_tls_repro_handler.wasm");
    for p in [&acceptor, &handler] {
        assert!(
            p.exists(),
            "missing {}. Build with: cd test-actors/close-notify-tls-repro && cargo build --release --target wasm32-unknown-unknown",
            p.display()
        );
    }
    (acceptor, handler)
}

fn write_repro_dir(cert_pem: &[u8], key_pem: &[u8]) {
    fs::create_dir_all(REPRO_DIR).expect("create repro dir");
    fs::write(format!("{}/cert.pem", REPRO_DIR), cert_pem).expect("write cert");
    fs::write(format!("{}/key.pem", REPRO_DIR), key_pem).expect("write key");
}

fn write_manifests(acceptor_wasm: &PathBuf, handler_wasm: &PathBuf) {
    let acceptor_toml = format!(
        r#"name = "close-notify-tls-repro-acceptor"
version = "0.1.0"
package = "{}"

[[handler]]
type = "runtime"

[[handler]]
type = "tcp"

[handler.server_tls]
enabled = true
cert = "{}/cert.pem"
key = "{}/key.pem"

[[handler]]
type = "supervisor"

[[handler]]
type = "rpc"
"#,
        acceptor_wasm.display(),
        REPRO_DIR,
        REPRO_DIR
    );
    let handler_toml = format!(
        r#"name = "close-notify-tls-repro-handler"
version = "0.1.0"
package = "{}"

[[handler]]
type = "runtime"

[[handler]]
type = "tcp"

[handler.server_tls]
enabled = true
cert = "{}/cert.pem"
key = "{}/key.pem"

[handler.client_tls]
enabled = true
skip_verify = true
auto_handshake = false
"#,
        handler_wasm.display(),
        REPRO_DIR,
        REPRO_DIR
    );
    fs::write(format!("{}/acceptor.toml", REPRO_DIR), acceptor_toml).expect("write acceptor.toml");
    fs::write(format!("{}/handler.toml", REPRO_DIR), handler_toml).expect("write handler.toml");
}

/// Minimal STARTTLS-aware SMTP-ish dummy. Mirrors what the inbox-side
/// smtp_deliver expects to see from the peer:
///   1. send 220 banner
///   2. read EHLO; reply 250-...250 STARTTLS
///   3. read STARTTLS; reply 220
///   4. upgrade to TLS (server side)
///   5. read EHLO over TLS; reply 250
///   6. read MAIL FROM; reply 250
///   7. read RCPT TO; reply 250
///   8. read DATA; reply 354
///   9. read body terminator; reply 250
///   10. read QUIT; reply 221
///   11. clean TLS shutdown
async fn spawn_dummy_outbound(cert_der: CertificateDer<'static>, key_der: PrivateKeyDer<'static>) {
    let server_cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .expect("server cfg");
    let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
    let listener = TcpListener::bind(("127.0.0.1", OUTBOUND_PORT))
        .await
        .expect("bind outbound");
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                // Plain phase.
                let _ = sock.write_all(b"220 dummy.local ESMTP probe\r\n").await;
                let _ = sock.read(&mut buf).await; // EHLO
                let _ = sock
                    .write_all(b"250-dummy.local hello\r\n250 STARTTLS\r\n")
                    .await;
                let _ = sock.read(&mut buf).await; // STARTTLS
                let _ = sock.write_all(b"220 ready\r\n").await;
                // Upgrade.
                let Ok(mut tls) = acceptor.accept(sock).await else {
                    return;
                };
                // Encrypted phase. Read each command, reply with the right code.
                for reply in [
                    "250 dummy.local hello\r\n", // EHLO
                    "250 ok\r\n",                 // MAIL FROM
                    "250 ok\r\n",                 // RCPT TO
                    "354 end with .\r\n",         // DATA
                    "250 queued\r\n",             // body + "."
                    "221 bye\r\n",                // QUIT
                ] {
                    let mut tmp = [0u8; 4096];
                    let _ = tls.read(&mut tmp).await;
                    let _ = tls.write_all(reply.as_bytes()).await;
                }
                let _ = tls.shutdown().await;
            });
        }
    });
}

async fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
#[ignore]
async fn close_notify_two_tls_streams_repro() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let (acceptor_wasm, handler_wasm) = ensure_wasm_built();

    // Fresh self-signed cert via rcgen.
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("rcgen");
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();
    write_repro_dir(cert_pem.as_bytes(), key_pem.as_bytes());
    write_manifests(&acceptor_wasm, &handler_wasm);

    // Dummy outbound TLS server using the same cert.
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::try_from(cert.key_pair.serialize_der()).expect("key parse");
    spawn_dummy_outbound(cert_der, key_der).await;
    assert!(
        wait_for_port(OUTBOUND_PORT, Duration::from_secs(2)).await,
        "outbound dummy didn't come up"
    );

    // Theater as a subprocess. Use the debug build the workspace just compiled.
    let theater_bin = workspace_root().join("target/debug/theater");
    assert!(
        theater_bin.exists(),
        "theater binary missing at {}; run `cargo build` first",
        theater_bin.display()
    );
    // Pipe theater's stderr to a log file so we can inspect close-handler
    // results after the test runs.
    let log_path = format!("{}/theater.log", REPRO_DIR);
    let log_file = std::fs::File::create(&log_path).expect("create log file");
    let log_file_err = log_file.try_clone().expect("dup log file");
    let mut theater = Command::new(&theater_bin)
        .args(["start", &format!("{}/acceptor.toml", REPRO_DIR)])
        .env("RUST_LOG", "theater_handler_tcp=debug,info")
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn()
        .expect("spawn theater");

    let ok = wait_for_port(INBOUND_PORT, Duration::from_secs(5)).await;
    if !ok {
        let _ = theater.kill();
        panic!("theater didn't bind 127.0.0.1:{} within 5s", INBOUND_PORT);
    }

    // Probe with strict rustls.
    let mut cfg = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny))
        .with_no_client_auth();
    cfg.alpn_protocols.clear();
    let connector = TlsConnector::from(Arc::new(cfg));
    let sock = tokio::net::TcpStream::connect(("127.0.0.1", INBOUND_PORT))
        .await
        .expect("connect");
    let server_name = ServerName::try_from("localhost").expect("name").to_owned();
    let mut tls = connector
        .connect(server_name, sock)
        .await
        .expect("tls connect");

    let body = r#"{"to":["x@y"],"subject":"probe","body":"hello"}"#;
    let req = format!(
        "POST /v1/send HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    tls.write_all(req.as_bytes()).await.expect("write req");

    let mut buf = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut buf)).await;

    // Teardown theater regardless of outcome.
    let _ = theater.kill();
    let _ = theater.wait();

    let r = read.expect("read timeout");
    let n = r.expect(
        "expected clean EOF (close_notify before FIN). \
         If UnexpectedEof here, ticket #10's prod bug has been pinned to a code path we can reproduce locally",
    );
    assert!(n > 0, "no response bytes");
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.contains("\"status\":\"sent\""),
        "response should contain status:sent — got: {}",
        text
    );
}
