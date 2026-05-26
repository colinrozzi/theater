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

/// Drive the inbox-CLI-shape HTTP client side: write request, then read
/// chunks until headers + Content-Length bytes are received, then BREAK
/// without further reads (no `read_to_end`). Mirrors inbox/cli/src/lib.rs
/// `http()` exactly — the same loop that surfaces `cli: recv: peer closed
/// connection without sending TLS close_notify` in production.
async fn inbox_cli_style_request(
    addr: std::net::SocketAddr,
    client_roots: RootCertStore,
    request_bytes: &[u8],
) -> Result<Vec<u8>, String> {
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

    tls.write_all(request_bytes).await.expect("client write");

    let mut all = Vec::new();
    let mut body_start: Option<usize> = None;
    let mut content_length: Option<usize> = None;

    loop {
        if let (Some(hs), Some(cl)) = (body_start, content_length) {
            if all.len() >= hs + cl {
                break;
            }
        }

        let mut chunk = [0u8; 65536];
        let read = tokio::time::timeout(Duration::from_secs(5), tls.read(&mut chunk)).await;
        let n = match read {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(format!("recv: {}", e)),
            Err(_) => return Err(String::from("recv: timeout")),
        };
        if n == 0 {
            break;
        }
        all.extend_from_slice(&chunk[..n]);

        if body_start.is_none() {
            let needle = b"\r\n\r\n";
            if let Some(idx) = all.windows(needle.len()).position(|w| w == needle) {
                body_start = Some(idx + needle.len());
                let header_str = core::str::from_utf8(&all[..idx]).unwrap_or("");
                for line in header_str.split("\r\n") {
                    if let Some((name, value)) = line.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("content-length") {
                            if let Ok(n) = value.trim().parse::<usize>() {
                                content_length = Some(n);
                            }
                        }
                    }
                }
                if content_length.is_none() {
                    content_length = Some(usize::MAX);
                }
            }
        }
    }

    Ok(all)
}

/// Server-side api-handler-shape lifecycle: accept TLS, one bounded
/// `read()` for the request, a brief processing pause (mimics the
/// synchronous side-effects inside inbox `handle_send`), write the
/// response, run the same close pattern as `theater-handler-tcp`'s
/// `close` host function.
async fn api_handler_shape_server(
    listener: TcpListener,
    server_cfg: ServerConfig,
    response_bytes: Vec<u8>,
    process_delay: Duration,
) {
    let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
    let (sock, _) = listener.accept().await.expect("accept");
    let tls = acceptor.accept(sock).await.expect("tls handshake");

    let stream_arc: Arc<Mutex<LocalState>> = Arc::new(Mutex::new(LocalState::Full(Box::new(tls))));

    // tcp_receive: one bounded read of the request.
    {
        let mut guard = stream_arc.lock().await;
        if let LocalState::Full(ref mut s) = *guard {
            let mut buf = [0u8; 65536];
            let _ = s.read(&mut buf).await;
        }
    }

    // Yield + sleep — mimic the processing pause between read and write.
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(process_delay).await;

    // tcp_send.
    {
        let mut guard = stream_arc.lock().await;
        if let LocalState::Full(ref mut s) = *guard {
            s.write_all(&response_bytes).await.expect("write_all");
        }
    }

    // tcp_close — same pattern as theater-handler-tcp/src/lib.rs `close`.
    {
        let mut guard = stream_arc.lock().await;
        let taken = std::mem::replace(&mut *guard, LocalState::Closed);
        drop(guard);
        match taken {
            LocalState::Full(mut s) => {
                let _ = AsyncWriteExt::shutdown(&mut *s).await;
            }
            LocalState::WriteOnly(mut w) => {
                let _ = AsyncWriteExt::shutdown(&mut w).await;
            }
            LocalState::Closed => {}
        }
    }
}

/// Repro target: a GET-shape request to an inbox-style handler, driven
/// by the same Content-Length-bounded read loop the inbox CLI uses.
/// Asserts the CLI-style loop completes without surfacing a "recv:" error
/// (i.e. without observing UnexpectedEof from rustls before its break point).
///
/// This is the experimental "is the minimal passive-mode pattern enough to
/// reproduce the prod close_notify warning?" test. If both this GET test
/// and the POST variant below pass on current main, the minimal shape
/// doesn't capture the prod bug and we need to add elements (outbound
/// SMTP-with-STARTTLS, wasm actor in the loop, etc).
#[tokio::test]
async fn inbox_cli_get_shape_does_not_observe_unexpected_eof() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let (server_cfg, client_roots) = loopback_tls();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    // Inbox-list-like response, JSON body, Content-Length set.
    let response_body =
        br#"[{"address":"theater-dev@colinrozzi.com","mailbox_id":"00000000-0000-0000-0000-000000000000"}]"#;
    let response = {
        let mut r = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        )
        .into_bytes();
        r.extend_from_slice(response_body);
        r
    };

    let server = tokio::spawn(api_handler_shape_server(
        listener,
        server_cfg,
        response,
        Duration::from_millis(20),
    ));

    let req = b"GET /v1/mailboxes HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer probe\r\nConnection: close\r\n\r\n";
    let body = inbox_cli_style_request(addr, client_roots, req)
        .await
        .expect("CLI-style GET should not surface 'recv:' error");

    server.await.expect("server task ok");

    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("theater-dev@colinrozzi.com"),
        "GET response should contain expected body — got: {}",
        text
    );
}

/// Repro target: the POST send shape — the exact prod path that surfaces
/// `cli: recv: peer closed connection without sending TLS close_notify`.
/// Same client read loop as the GET variant, but with a request body and
/// a response shape that matches inbox `/send`.
///
/// On current main: if this test FAILS with `recv: ...`, we've finally
/// reproduced the prod symptom in a Cargo test. The same test then locks
/// in the eventual fix.
///
/// If it PASSES, the minimal pattern still doesn't capture the prod bug
/// — next iteration adds the concurrent outbound SMTP-with-STARTTLS
/// (handle_send opens a separate TLS connection mid-processing).
#[tokio::test]
async fn inbox_cli_post_send_shape_does_not_observe_unexpected_eof() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let (server_cfg, client_roots) = loopback_tls();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    // Inbox /send response shape (PR #4), small JSON body.
    let response_body =
        br#"{"status":"sent","delivered":["theater-dev@colinrozzi.com"],"failed":[]}"#;
    let response = {
        let mut r = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        )
        .into_bytes();
        r.extend_from_slice(response_body);
        r
    };

    // The processing pause for POST send is longer than a typical GET —
    // smtp_deliver synchronously does TCP+TLS to localhost:25. Approximate
    // with a wider sleep here.
    let server = tokio::spawn(api_handler_shape_server(
        listener,
        server_cfg,
        response,
        Duration::from_millis(150),
    ));

    let req_body =
        br#"{"to":["theater-dev@colinrozzi.com"],"cc":[],"bcc":[],"subject":"probe","body":"x"}"#;
    let mut req = format!(
        "POST /v1/mailboxes/theater-dev%40colinrozzi.com/send HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer probe\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        req_body.len()
    )
    .into_bytes();
    req.extend_from_slice(req_body);

    let body = inbox_cli_style_request(addr, client_roots, &req)
        .await
        .expect(
            "CLI-style POST should not surface 'recv:' error — \
             if this fails with UnexpectedEof, we've reproduced the prod close_notify warning",
        );

    server.await.expect("server task ok");

    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("\"status\":\"sent\""),
        "POST response should contain status:sent — got: {}",
        text
    );
}

/// Tiny SMTP-shaped TLS server: accept TLS, write a 220-style banner,
/// read a short EHLO line, write a 250 response, run the same close
/// pattern as the api handler. Used as the outbound target for
/// `inbox_cli_post_send_with_outbound_tls_*` — the closest minimal
/// stand-in for the inbox `smtp_deliver` step that distinguishes POST
/// from GET in production.
async fn mini_smtp_tls_target(listener: TcpListener, server_cfg: ServerConfig) {
    let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
    let (sock, _) = listener.accept().await.expect("smtp accept");
    let mut tls = match acceptor.accept(sock).await {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = tls.write_all(b"220 ready\r\n").await;
    let mut buf = [0u8; 4096];
    let _ = tls.read(&mut buf).await;
    let _ = tls.write_all(b"250 ok\r\n").await;
    let _ = AsyncWriteExt::shutdown(&mut tls).await;
}

/// Iteration 4a (per the PR description): re-run the POST send shape, but
/// have the api-handler-shape server open an outbound TLS connection to
/// a second in-test TLS server BETWEEN the request read and the response
/// write — the closest minimal stand-in for inbox `smtp_deliver`. If the
/// concurrent outbound TLS during processing is what perturbs the inbound
/// connection's close path, this is where it surfaces.
#[tokio::test]
async fn inbox_cli_post_send_with_outbound_tls_does_not_observe_unexpected_eof() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Inbound TLS server (the one the client connects to).
    let (in_server_cfg, in_client_roots) = loopback_tls();
    let in_listener = TcpListener::bind("127.0.0.1:0").await.expect("in bind");
    let in_addr = in_listener.local_addr().expect("in local addr");

    // Outbound TLS target (mini SMTP-shaped) — separate cert chain so the
    // api-handler's outbound rustls connection is a fully independent
    // session, same as inbox's outbound to localhost:25.
    let (out_server_cfg, out_client_roots) = loopback_tls();
    let out_listener = TcpListener::bind("127.0.0.1:0").await.expect("out bind");
    let out_addr = out_listener.local_addr().expect("out local addr");

    let smtp_handle = tokio::spawn(mini_smtp_tls_target(out_listener, out_server_cfg));

    // Server task: api-handler shape inlined with the outbound TLS step
    // wedged in between request read and response write.
    let response_body =
        br#"{"status":"sent","delivered":["theater-dev@colinrozzi.com"],"failed":[]}"#;
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response_body.len()
    )
    .into_bytes();
    response.extend_from_slice(response_body);

    let server = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(in_server_cfg));
        let (sock, _) = in_listener.accept().await.expect("in accept");
        let tls = acceptor.accept(sock).await.expect("in tls handshake");
        let stream_arc: Arc<Mutex<LocalState>> =
            Arc::new(Mutex::new(LocalState::Full(Box::new(tls))));

        // tcp_receive: one bounded read of the request.
        {
            let mut guard = stream_arc.lock().await;
            if let LocalState::Full(ref mut s) = *guard {
                let mut buf = [0u8; 65536];
                let _ = s.read(&mut buf).await;
            }
        }

        // PROCESSING STEP: concurrent outbound TLS — the smtp_deliver
        // stand-in. tcp_connect (plain) → tls handshake → write EHLO →
        // read 250 → AsyncWriteExt::shutdown. This is the element the
        // minimal POST test was missing.
        {
            let out_client_cfg = ClientConfig::builder()
                .with_root_certificates(out_client_roots)
                .with_no_client_auth();
            let connector = TlsConnector::from(Arc::new(out_client_cfg));
            let sock = tokio::net::TcpStream::connect(out_addr)
                .await
                .expect("outbound tcp connect");
            let server_name = ServerName::try_from("localhost").expect("name").to_owned();
            let mut out_tls = connector
                .connect(server_name, sock)
                .await
                .expect("outbound tls handshake");

            let mut banner = [0u8; 64];
            let _ = out_tls.read(&mut banner).await;
            let _ = out_tls.write_all(b"EHLO probe\r\n").await;
            let mut resp = [0u8; 64];
            let _ = out_tls.read(&mut resp).await;
            let _ = AsyncWriteExt::shutdown(&mut out_tls).await;
        }

        // tcp_send response.
        {
            let mut guard = stream_arc.lock().await;
            if let LocalState::Full(ref mut s) = *guard {
                s.write_all(&response).await.expect("write_all");
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
                LocalState::WriteOnly(mut w) => {
                    let _ = AsyncWriteExt::shutdown(&mut w).await;
                }
                LocalState::Closed => {}
            }
        }
    });

    let req_body =
        br#"{"to":["theater-dev@colinrozzi.com"],"cc":[],"bcc":[],"subject":"probe","body":"x"}"#;
    let mut req = format!(
        "POST /v1/mailboxes/theater-dev%40colinrozzi.com/send HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer probe\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        req_body.len()
    )
    .into_bytes();
    req.extend_from_slice(req_body);

    let body = inbox_cli_style_request(in_addr, in_client_roots, &req)
        .await
        .expect(
            "POST + outbound TLS during processing should not surface 'recv:' — \
         if this fails, the concurrent outbound TLS IS the trigger",
        );

    server.await.expect("server task ok");
    smtp_handle.await.expect("smtp task ok");

    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("\"status\":\"sent\""),
        "POST response should contain status:sent — got: {}",
        text
    );
}

/// Pattern documentation: the active-mode reader's peer-EOF cleanup must
/// flush close_notify on the held write half before dropping it.
///
/// **Pattern-doc, not integration:** like the other two tests in this file,
/// this is a local stand-in that documents the *correct* cleanup pattern.
/// It does NOT call into production `tcp_read_loop` or
/// `shutdown_write_half_and_remove` — it constructs its own server task and
/// runs the equivalent inline. End-to-end verification against the actual
/// production code lives at deploy-time: the inbox CLI is a strict rustls
/// client talking to a real theater-handler-tcp server, so its self-send
/// probe (no `peer closed connection without sending TLS close_notify`
/// warnings) is the integration check.
///
/// What the pattern is: when an actor sets a connection to active mode,
/// theater-handler-tcp splits the TLS stream via `tokio::io::split`, parks
/// the write half in `StreamState::WriteOnly(write_half)` inside the shared
/// connections map, and spawns `tcp_read_loop` on the read half. When the
/// peer initiates a clean TLS shutdown (close_notify + FIN), the reader
/// sees `Ok(0)`; the cleanup branch must take the WriteOnly write half and
/// call `AsyncWriteExt::shutdown` on it before removing the entry from the
/// map (which drops the write half). Without the shutdown call, the rustls
/// session is dropped before its outgoing close_notify alert is flushed,
/// and peers observe a bare TCP FIN — `UnexpectedEof` from the rustls
/// client side, which is the symptom every `inbox send` had been surfacing
/// as `cli: recv: peer closed connection without sending TLS close_notify`.
///
/// Mirror this pattern (including the `AsyncWriteExt::shutdown` call) when
/// adding any future cleanup path that owns a TLS write half.
#[tokio::test]
async fn active_mode_eof_cleanup_pattern_sends_close_notify() {
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
