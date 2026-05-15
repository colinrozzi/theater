//! Acceptor for the close_notify-on-concurrent-TLS-streams repro.
//!
//! Listens on a fixed TLS port; on each accept, spawns a handler actor and
//! transfers the connection to it. Mirrors the inbox acceptor topology.

#![no_std]
extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, GraphValue, Value, ValueType};

packr_guest::setup_guest!();

#[derive(Clone, GraphValue)]
#[graph(crate = "packr_guest::composite_abi")]
pub struct AcceptorState {
    pub listener_id: String,
    pub handler_manifest: String,
}

pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/tcp {
            listen: func(address: string) -> result<string, string>,
            transfer: func(connection-id: string, target-actor: string) -> result<_, string>,
        }
        theater:simple/supervisor {
            spawn: func(manifest: string, init-bytes: option<list<u8>>, wasm-bytes: option<list<u8>>) -> result<string, string>,
        }
        theater:simple/rpc {
            call: func(actor-id: string, function: string, params: value, options: value) -> value,
        }
    }
    exports {
        theater:simple/actor.init: func(state: value) -> result<acceptor-state, string>,
        theater:simple/tcp-client.handle-connection: func(state: acceptor-state, connection-id: string) -> result<acceptor-state, string>,
    }
}

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/tcp", name = "listen")]
fn tcp_listen(address: String) -> Result<String, String>;

#[import(module = "theater:simple/tcp", name = "transfer")]
fn tcp_transfer(connection_id: String, target_actor: String) -> Result<(), String>;

#[import(module = "theater:simple/supervisor", name = "spawn")]
fn supervisor_spawn(
    manifest: String,
    init_bytes: Option<Vec<u8>>,
    wasm_bytes: Option<Vec<u8>>,
) -> Result<String, String>;

#[import(module = "theater:simple/rpc", name = "call")]
fn rpc_call(actor_id: String, function: String, params: Value, options: Value) -> Value;

const LISTEN_ADDR: &str = "127.0.0.1:18443";
const HANDLER_MANIFEST: &str = "/tmp/close_notify_repro/handler.toml";

#[export(name = "theater:simple/actor.init")]
fn init(_state: Value) -> Result<(AcceptorState, ()), String> {
    log(String::from("[repro-acceptor] init"));

    let listener_id = tcp_listen(String::from(LISTEN_ADDR))
        .map_err(|e| format!("listen failed: {}", e))?;
    log(format!(
        "[repro-acceptor] listening on {} (id={})",
        LISTEN_ADDR, listener_id
    ));

    Ok((
        AcceptorState {
            listener_id,
            handler_manifest: String::from(HANDLER_MANIFEST),
        },
        (),
    ))
}

#[export(name = "theater:simple/tcp-client.handle-connection")]
fn handle_connection(
    state: AcceptorState,
    connection_id: String,
) -> Result<(AcceptorState, ()), String> {
    log(format!("[repro-acceptor] new connection {}", connection_id));

    let handler_id = supervisor_spawn(state.handler_manifest.clone(), None, None)
        .map_err(|e| format!("spawn handler failed: {}", e))?;
    log(format!("[repro-acceptor] spawned handler {}", handler_id));

    // supervisor.spawn doesn't auto-init — RPC the handler's init explicitly.
    let init_params = Value::Tuple(alloc::vec![Value::Option {
        inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
        value: None,
    }]);
    let _ = rpc_call(
        handler_id.clone(),
        String::from("theater:simple/actor.init"),
        init_params,
        Value::Tuple(alloc::vec![]),
    );

    tcp_transfer(connection_id, handler_id).map_err(|e| format!("transfer failed: {}", e))?;

    Ok((state, ()))
}
