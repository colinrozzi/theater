//! Loopback regression test for graceful TLS shutdown on `close()`.
//!
//! Mirrors the inbox api-handler pattern: server accepts a TLS connection,
//! writes a response, calls `AsyncWriteExt::shutdown` (the same call the
//! tcp handler's `close` host function performs), then drops the stream.
//!
//! The strict rustls client we connect with reports
//! `UnexpectedEof` if the server didn't send `close_notify` before FIN.
//! That's exactly the symptom the inbox cli surfaces — so this test fails
//! if `close()`'s shutdown path is broken, and passes once it isn't.

use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_rustls::server::TlsStream as ServerTlsStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// Local stand-in for theater_handler_tcp::StreamState — same shape (Box +
/// enum + Closed sentinel), so the close path exercises the same drop
/// ordering and Box deref the production handler does.
enum LocalState {
    Full(Box<ServerTlsStream<tokio::net::TcpStream>>),
    Closed,
}

/// Build a server tls config + matching client root store from a fresh
/// self-signed cert. The cert is only valid for "localhost".
fn loopback_tls() -> (ServerConfig, RootCertStore) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("rcgen ok");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::try_from(cert.key_pair.serialize_der()).expect("key parse ok");

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der)
        .expect("server cfg");

    let mut roots = RootCertStore::empty();
    roots.add(cert_der).expect("add cert to roots");

    (server_config, roots)
}

/// Reproduce the inbox api-handler's per-response close pattern and verify
/// a strict rustls client sees clean EOF (i.e. close_notify was flushed).
#[tokio::test]
async fn server_shutdown_sends_close_notify() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let (server_cfg, client_roots) = loopback_tls();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    // Mimic the production flow:
    //   1. Accept task wraps the stream in Arc<Mutex<StreamState::Full(Box<...>)>>
    //   2. "Send" task locks the mutex, write_all on the boxed stream, releases lock
    //   3. "Close" task locks, std::mem::replace with Closed, drops guard, then
    //      AsyncWriteExt::shutdown(&mut *boxed_stream).await
    //   4. The taken StreamState (and the Box) drop at end of match arm.
    let server = tokio::spawn({
        let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
        async move {
            let (sock, _) = listener.accept().await.expect("accept");
            let tls = acceptor.accept(sock).await.expect("tls handshake");

            let stream_arc: Arc<Mutex<LocalState>> =
                Arc::new(Mutex::new(LocalState::Full(Box::new(tls))));

            // Drain the client's request — mimic tcp_receive's bounded read.
            {
                let mut guard = stream_arc.lock().await;
                if let LocalState::Full(ref mut s) = *guard {
                    let mut buf = [0u8; 4096];
                    let _ = s.read(&mut buf).await;
                }
            }

            // "send" the response
            {
                let mut guard = stream_arc.lock().await;
                if let LocalState::Full(ref mut s) = *guard {
                    s.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                    )
                    .await
                    .expect("write_all");
                }
            }

            // "close" — exact pattern from theater-handler-tcp/src/lib.rs close()
            {
                let mut guard = stream_arc.lock().await;
                let taken = std::mem::replace(&mut *guard, LocalState::Closed);
                drop(guard);
                match taken {
                    LocalState::Full(mut s) => {
                        let _ = AsyncWriteExt::shutdown(&mut *s).await;
                    }
                    LocalState::Closed => {}
                }
            }
        }
    });

    // Client: strict rustls (default), connect, read to EOF, observe whether
    // the EOF was preceded by close_notify.
    let client_cfg = ClientConfig::builder()
        .with_root_certificates(client_roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(client_cfg));
    let sock = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let server_name = ServerName::try_from("localhost").expect("name").to_owned();
    let mut tls = connector
        .connect(server_name, sock)
        .await
        .expect("tls connect");

    // Client sends an HTTP-ish request first (mimic the cli http path).
    tls.write_all(b"GET /v1/ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("client write");

    let mut buf = Vec::new();
    // Give the server time to finish writing + shutting down.
    let read = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut buf)).await;

    server.await.expect("server task ok");

    let result = read.expect("read timed out");
    let n = result.expect(
        "expected clean EOF (close_notify before FIN); got UnexpectedEof — \
         server tcp close handler did not flush close_notify before dropping stream",
    );
    assert!(n > 0, "expected response bytes, got empty");
    assert!(
        buf.windows(5).any(|w| w == b"hello"),
        "response should contain 'hello' body"
    );
}
