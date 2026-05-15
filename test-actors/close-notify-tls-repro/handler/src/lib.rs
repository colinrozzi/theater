//! Handler for the close_notify-on-concurrent-TLS-streams repro.
//!
//! Receives a transferred inbound TLS connection, then opens a SECOND
//! outbound connection that goes through a STARTTLS upgrade — the exact
//! shape of the inbox `smtp_deliver` path that ticket #10 tracks:
//!
//!   tcp_connect (plain) -> banner -> EHLO -> STARTTLS ->
//!   upgrade-to-tls-client -> EHLO over TLS -> MAIL FROM -> RCPT TO ->
//!   DATA -> body -> "." -> QUIT -> tcp_close
//!
//! Then writes the response on the inbound and runs the close+shutdown
//! sequence. Concurrent two-TLS-streams pattern, where the second stream
//! crossed an explicit upgrade boundary — that's the variable ticket #10's
//! prior local repro (auto-handshake on connect) didn't exercise.

#![no_std]
extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, GraphValue, Value};

packr_guest::setup_guest!();

#[derive(Clone, GraphValue)]
#[graph(crate = "packr_guest::composite_abi")]
pub struct HandlerState;

pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
            shutdown: func(data: option<list<u8>>) -> result<_, string>,
        }
        theater:simple/tcp {
            connect: func(address: string) -> result<string, string>,
            receive: func(connection-id: string, max-bytes: u32) -> result<list<u8>, string>,
            send: func(connection-id: string, data: list<u8>) -> result<u64, string>,
            close: func(connection-id: string) -> result<_, string>,
            upgrade-to-tls-client: func(connection-id: string, server-name: string) -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<handler-state, string>,
        theater:simple/tcp-client.handle-connection-transfer: func(state: handler-state, connection-id: string) -> result<handler-state, string>,
    }
}

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/runtime", name = "shutdown")]
fn shutdown(data: Option<Vec<u8>>) -> Result<(), String>;

#[import(module = "theater:simple/tcp", name = "connect")]
fn tcp_connect(address: String) -> Result<String, String>;

#[import(module = "theater:simple/tcp", name = "receive")]
fn tcp_receive(connection_id: String, max_bytes: u32) -> Result<Vec<u8>, String>;

#[import(module = "theater:simple/tcp", name = "send")]
fn tcp_send(connection_id: String, data: Vec<u8>) -> Result<u64, String>;

#[import(module = "theater:simple/tcp", name = "close")]
fn tcp_close(connection_id: String) -> Result<(), String>;

#[import(module = "theater:simple/tcp", name = "upgrade-to-tls-client")]
fn tcp_upgrade_to_tls_client(connection_id: String, server_name: String) -> Result<(), String>;

const OUTBOUND_ADDR: &str = "127.0.0.1:18444";
const OUTBOUND_SNI: &str = "localhost";
const RESPONSE_BODY: &str = r#"{"status":"sent","delivered":["x@y"],"failed":[]}"#;

fn outbound_smtp_session() -> Result<(), String> {
    let conn = tcp_connect(String::from(OUTBOUND_ADDR))
        .map_err(|e| format!("connect: {}", e))?;
    log(format!("[repro-handler] outbound connected: {}", conn));

    // banner (plain)
    let _ = tcp_receive(conn.clone(), 1024).map_err(|e| format!("banner read: {}", e))?;
    // EHLO (plain)
    let _ = tcp_send(conn.clone(), b"EHLO probe.local\r\n".to_vec());
    let _ = tcp_receive(conn.clone(), 1024);
    // STARTTLS
    let _ = tcp_send(conn.clone(), b"STARTTLS\r\n".to_vec());
    let _ = tcp_receive(conn.clone(), 1024);
    // Upgrade to TLS — the load-bearing call for ticket #10.
    tcp_upgrade_to_tls_client(conn.clone(), String::from(OUTBOUND_SNI))
        .map_err(|e| format!("upgrade-to-tls-client: {}", e))?;
    log(String::from("[repro-handler] STARTTLS upgrade ok"));
    // EHLO (encrypted)
    let _ = tcp_send(conn.clone(), b"EHLO probe.local\r\n".to_vec());
    let _ = tcp_receive(conn.clone(), 1024);
    // MAIL FROM
    let _ = tcp_send(conn.clone(), b"MAIL FROM:<probe@local>\r\n".to_vec());
    let _ = tcp_receive(conn.clone(), 1024);
    // RCPT TO
    let _ = tcp_send(conn.clone(), b"RCPT TO:<dest@local>\r\n".to_vec());
    let _ = tcp_receive(conn.clone(), 1024);
    // DATA
    let _ = tcp_send(conn.clone(), b"DATA\r\n".to_vec());
    let _ = tcp_receive(conn.clone(), 1024);
    let _ = tcp_send(
        conn.clone(),
        b"From: probe@local\r\nTo: dest@local\r\nSubject: hi\r\n\r\nhello\r\n.\r\n".to_vec(),
    );
    let _ = tcp_receive(conn.clone(), 1024);
    // QUIT
    let _ = tcp_send(conn.clone(), b"QUIT\r\n".to_vec());
    let _ = tcp_receive(conn.clone(), 1024);
    let _ = tcp_close(conn);
    Ok(())
}

#[export(name = "theater:simple/actor.init")]
fn init(_state: Value) -> Result<(HandlerState, ()), String> {
    Ok((HandlerState, ()))
}

#[export(name = "theater:simple/tcp-client.handle-connection-transfer")]
fn handle_connection_transfer(
    state: HandlerState,
    connection_id: String,
) -> Result<(HandlerState, ()), String> {
    log(format!("[repro-handler] got conn {}", connection_id));

    // Drain the inbound request.
    let _ = tcp_receive(connection_id.clone(), 8192);

    if let Err(e) = outbound_smtp_session() {
        log(format!("[repro-handler] outbound session failed: {}", e));
    }

    // Respond on the inbound — same shape as inbox /send.
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        RESPONSE_BODY.len(),
        RESPONSE_BODY
    );
    if let Err(e) = tcp_send(connection_id.clone(), response.into_bytes()) {
        log(format!("[repro-handler] inbound send failed: {}", e));
    }
    let _ = tcp_close(connection_id);

    let _ = shutdown(None);

    Ok((state, ()))
}
