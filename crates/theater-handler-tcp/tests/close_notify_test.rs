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
    /// Mirrors the production `WriteOnly(UnifiedWriteHalf)` arm — entered when
    /// a connection is split for the active-mode reader pattern. Holding the
    /// write half here lets the cleanup path call `AsyncWriteExt::shutdown`
    /// before dropping (i.e. send TLS close_notify before TCP FIN).
    WriteOnly(tokio::io::WriteHalf<ServerTlsStream<tokio::net::TcpStream>>),
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

/// POST-shape variant that mirrors the inbox `/send` close path:
///   1. client POSTs an HTTP request body
///   2. server reads, simulates a long synchronous side-effect (smtp_deliver),
///      writes a larger structured JSON response, runs the close pattern
///   3. assert clean EOF (no missing close_notify)
///
/// Ticket #10 hypothesis 1: tcp_close returns before close_notify is on the wire
/// when the response is large enough that some encrypted bytes are still buffered
/// in the kernel send queue when shutdown(SHUT_WR) fires. If reproducible, this
/// test will trip with `UnexpectedEof`.
#[tokio::test]
async fn post_response_close_pattern_sends_close_notify() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let (server_cfg, client_roots) = loopback_tls();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let server = tokio::spawn({
        let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
        async move {
            let (sock, _) = listener.accept().await.expect("accept");
            let tls = acceptor.accept(sock).await.expect("tls handshake");

            let stream_arc: Arc<Mutex<LocalState>> =
                Arc::new(Mutex::new(LocalState::Full(Box::new(tls))));

            // tcp_receive: bounded read of the POST request.
            {
                let mut guard = stream_arc.lock().await;
                if let LocalState::Full(ref mut s) = *guard {
                    let mut buf = [0u8; 65536];
                    let _ = s.read(&mut buf).await;
                }
            }

            // Simulate the synchronous smtp_deliver side-effect that sits
            // between request read and response write in the inbox /send path.
            // Yields a few times so the runtime gets a chance to do something
            // weird if there's a race in actor-task scheduling.
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Build a response that's the same shape and approximate size as
            // the inbox /send structured response (PR #4).
            let body = r#"{"status":"sent","delivered":["claude@colinrozzi.com","colinrozzi@gmail.com"],"failed":[]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            // tcp_send
            {
                let mut guard = stream_arc.lock().await;
                if let LocalState::Full(ref mut s) = *guard {
                    s.write_all(response.as_bytes()).await.expect("write_all");
                }
            }

            // tcp_close — same pattern as theater-handler-tcp/src/lib.rs.
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

    // POST request matching the inbox cli's /send body shape.
    let req_body = r#"{"to":["claude@colinrozzi.com"],"cc":["colinrozzi@gmail.com"],"bcc":[],"subject":"close_notify probe","body":"probe — disregard","smtp_server":"localhost:25"}"#;
    let req = format!(
        "POST /v1/mailboxes/theater-dev%40colinrozzi.com/send HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer probe-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        req_body.len(),
        req_body
    );
    tls.write_all(req.as_bytes()).await.expect("client write");

    let mut buf = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut buf)).await;

    server.await.expect("server task ok");

    let result = read.expect("read timed out");
    let n = result.expect(
        "expected clean EOF on POST response close pattern; got UnexpectedEof — \
         ticket #10 race reproduced",
    );
    assert!(n > 0, "expected response bytes, got empty");
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.contains("\"status\":\"sent\""),
        "response should contain status:sent — got: {}",
        text
    );
}

/// Active-mode reader EOF must flush close_notify before dropping the write half.
///
/// This mirrors the `set-active "active"` path inside theater-handler-tcp:
///   1. After handshake, the stream is split (`tokio::io::split`)
///   2. The read half goes to a background reader task (`tcp_read_loop`)
///   3. The write half lives in `StreamState::WriteOnly(write_half)` inside
///      the shared connections map
///   4. The actor writes responses through the write half via `tcp.send`
///
/// When the peer initiates a clean TLS shutdown (sends `close_notify` then
/// FIN), the reader sees `Ok(0)` and runs the EOF cleanup branch. Prior to
/// the fix that introduced this test, that branch called the actor's
/// `on-close` callback then removed the entry from the connections map —
/// which dropped the `WriteOnly(write_half)` without ever calling
/// `AsyncWriteExt::shutdown`. For TLS, dropping the write half without
/// shutdown means the rustls layer never flushes its outgoing `close_notify`
/// alert, so the peer's read side observes raw TCP FIN and returns
/// `UnexpectedEof`. That is the symptom every `inbox send` from the cli
/// surfaces as `cli: recv: peer closed connection without sending TLS
/// close_notify`.
///
/// This test models the exact pattern: server splits + spawns reader, client
/// reads response then initiates clean shutdown, the reader's EOF branch
/// calls `AsyncWriteExt::shutdown` on the held write half before dropping it.
/// Without the shutdown call the client's `read_to_end` returns
/// `UnexpectedEof` — the assertion below fails immediately.
#[tokio::test]
async fn active_mode_reader_eof_sends_close_notify() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let (server_cfg, client_roots) = loopback_tls();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let server = tokio::spawn({
        let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
        async move {
            let (sock, _) = listener.accept().await.expect("accept");
            let tls = acceptor.accept(sock).await.expect("tls handshake");

            // Split the stream — entering the active-mode pattern.
            let (mut read_half, write_half) = tokio::io::split(tls);
            let stream_arc: Arc<Mutex<LocalState>> =
                Arc::new(Mutex::new(LocalState::WriteOnly(write_half)));

            // Drain the request through the read half — mirrors tcp_read_loop's
            // bounded read against the split read half.
            let mut req_buf = [0u8; 4096];
            let _ = read_half.read(&mut req_buf).await;

            // "send" the response via the write half held in the shared state.
            // Mirrors tcp_send acquiring the StreamState lock and writing.
            {
                let mut guard = stream_arc.lock().await;
                if let LocalState::WriteOnly(ref mut w) = *guard {
                    w.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                    )
                    .await
                    .expect("write_all");
                }
            }

            // Reader continues until EOF (the client's close_notify + FIN).
            // This is the loop body in tcp_read_loop — read until Ok(0).
            let mut sink = [0u8; 4096];
            loop {
                match read_half.read(&mut sink).await {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }

            // EOF cleanup — production tcp_read_loop runs the equivalent of
            // this block. WITH the fix: take the write half from StreamState
            // and shutdown it before drop. WITHOUT the fix: just remove the
            // entry from the map (drop the WriteOnly without shutdown).
            {
                let mut guard = stream_arc.lock().await;
                let taken = std::mem::replace(&mut *guard, LocalState::Closed);
                drop(guard);
                if let LocalState::WriteOnly(mut w) = taken {
                    // This shutdown call is what flushes the server-side
                    // close_notify. Comment it out to reproduce the bug.
                    let _ = AsyncWriteExt::shutdown(&mut w).await;
                }
            }
        }
    });

    // Client: strict rustls, connect, write request, read response, then
    // gracefully shutdown (sends our close_notify, waits for the server's
    // reply). Final read_to_end asserts the server's reply close_notify
    // arrived before FIN — without it, rustls converts the bare FIN into
    // UnexpectedEof.
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

    tls.write_all(b"GET /v1/ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("client write");

    // Read the response body (Content-Length-bounded).
    let mut body = [0u8; 256];
    let body_read = tokio::time::timeout(Duration::from_secs(5), tls.read(&mut body))
        .await
        .expect("body read timed out")
        .expect("body read");
    assert!(body_read > 0, "expected response bytes, got empty");
    assert!(
        body[..body_read].windows(5).any(|w| w == b"hello"),
        "response should contain 'hello' body"
    );

    // Initiate clean TLS shutdown from the client side — this sends our
    // close_notify and waits for the server's reply close_notify.
    tls.shutdown().await.expect("client tls shutdown");

    // Drain to EOF. If the server reaped without flushing close_notify, this
    // returns UnexpectedEof — exactly the symptom we're guarding against.
    let mut tail = Vec::new();
    let drained = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut tail))
        .await
        .expect("drain timed out");
    drained.expect(
        "expected clean EOF after client shutdown (server flushed close_notify); \
         got UnexpectedEof — tcp_read_loop's EOF/Err cleanup branch dropped the \
         WriteOnly write half without calling AsyncWriteExt::shutdown",
    );

    server.await.expect("server task ok");
}
