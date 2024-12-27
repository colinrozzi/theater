use crate::actor_runtime::ChainRequest;
use crate::actor_runtime::ChainRequestType;
use crate::store::Store;
use crate::wasm::Event;
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::future::Future;
use tokio::sync::mpsc::Sender;
use tracing::info;
use wasmtime::component::{Component, ComponentExportIndex, Linker};

pub enum Capability {
    Base(BaseCapability),
    Http(HttpCapability),
}

impl Capability {
    pub async fn setup_host_functions(&self, linker: &mut Linker<Store>) -> Result<()> {
        match self {
            Capability::Base(capability) => capability.setup_host_functions(linker).await,
            Capability::Http(capability) => capability.setup_host_functions(linker).await,
        }
    }

    pub fn get_exports(
        &self,
        component: &Component,
    ) -> Result<Vec<(String, ComponentExportIndex)>> {
        match self {
            Capability::Base(capability) => capability.get_exports(component),
            Capability::Http(capability) => capability.get_exports(component),
        }
    }

    pub fn interface_name(&self) -> &str {
        match self {
            Capability::Base(capability) => capability.interface_name(),
            Capability::Http(capability) => capability.interface_name(),
        }
    }
}

/// The base actor capability that all actors must implement
pub struct BaseCapability;

impl BaseCapability {
    async fn setup_host_functions(&self, linker: &mut Linker<Store>) -> Result<()> {
        let mut runtime = linker.instance("ntwk:simple-actor/runtime")?;

        // Add log function
        runtime.func_wrap(
            "log",
            |_: wasmtime::StoreContextMut<'_, Store>, (msg,): (String,)| {
                log(msg);
                Ok(())
            },
        )?;

        // Add send function
        runtime.func_wrap(
            "send",
            |mut ctx: wasmtime::StoreContextMut<'_, Store>, (address, msg): (String, Vec<u8>)| {
                let store = ctx.data_mut();
                send(store, address, msg);
                Ok(())
            },
        )?;

        Ok(())
    }

    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>> {
        let (_, instance) = component
            .export_index(None, "ntwk:simple-actor/actor")
            .expect("Failed to get actor instance");

        let mut exports = Vec::new();

        // Get required function exports
        let (_, init) = component
            .export_index(Some(&instance), "init")
            .expect("Failed to get init export");
        exports.push(("init".to_string(), init));

        let (_, handle) = component
            .export_index(Some(&instance), "handle")
            .expect("Failed to get handle export");
        exports.push(("handle".to_string(), handle));

        let (_, state_contract) = component
            .export_index(Some(&instance), "state-contract")
            .expect("Failed to get state contract export");
        exports.push(("state-contract".to_string(), state_contract));

        let (_, message_contract) = component
            .export_index(Some(&instance), "message-contract")
            .expect("Failed to get message contract export");
        exports.push(("message-contract".to_string(), message_contract));

        Ok(exports)
    }

    fn interface_name(&self) -> &str {
        "ntwk:simple-actor/actor"
    }
}

/// HTTP actor capability
pub struct HttpCapability;

impl HttpCapability {
    async fn setup_host_functions(&self, linker: &mut Linker<Store>) -> Result<()> {
        let mut runtime = linker.instance("ntwk:simple-http-actor/http-runtime")?;

        // Add log function
        runtime.func_wrap(
            "log",
            |_: wasmtime::StoreContextMut<'_, Store>, (msg,): (String,)| {
                log(msg);
                Ok(())
            },
        )?;

        // Add send function - reuse same implementation as BaseActorCapability
        runtime.func_wrap(
            "send",
            |mut ctx: wasmtime::StoreContextMut<'_, Store>, (address, msg): (String, Vec<u8>)| {
                let store = ctx.data_mut();
                send(store, address, msg);
                Ok(())
            },
        )?;

        runtime.func_wrap_async(
            "http-send",
            |mut ctx: wasmtime::StoreContextMut<'_, Store>,
             (address, msg): (String, Vec<u8>)|
             -> Box<dyn Future<Output = Result<(Vec<u8>,)>> + Send> {
                let store = ctx.data_mut();
                let chain_tx = store.chain_tx.clone();
                Box::new(http_send(chain_tx, address, msg))
            },
        )?;

        Ok(())
    }

    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>> {
        let (_, instance) = component
            .export_index(None, "ntwk:simple-http-actor/actor")
            .expect("Failed to get actor instance");

        let mut exports = Vec::new();
        // Get required function exports
        let (_, init) = component
            .export_index(Some(&instance), "init")
            .expect("Failed to get init export");
        exports.push(("init".to_string(), init));

        let (_, handle) = component
            .export_index(Some(&instance), "handle")
            .expect("Failed to get handle export");
        exports.push(("handle".to_string(), handle));

        let (_, state_contract) = component
            .export_index(Some(&instance), "state-contract")
            .expect("Failed to get state contract export");
        exports.push(("state-contract".to_string(), state_contract));

        let (_, message_contract) = component
            .export_index(Some(&instance), "message-contract")
            .expect("Failed to get message contract export");
        exports.push(("message-contract".to_string(), message_contract));

        Ok(exports)
    }

    fn interface_name(&self) -> &str {
        "ntwk:simple-http-actor/actor"
    }
}

fn log(msg: String) {
    info!("[ACTOR] {}", msg);
}

fn send(store: &Store, address: String, msg: Vec<u8>) {
    let msg_value: Value = serde_json::from_slice(&msg).expect("Failed to parse message as JSON");
    let evt = Event {
        type_: "actor-message".to_string(),
        data: json!({
            "address": address,
            "message": msg_value,
        }),
    };

    let chain_tx = store.chain_tx.clone();

    let _result = tokio::spawn(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();

        chain_tx
            .send(ChainRequest {
                request_type: ChainRequestType::AddEvent { event: evt },
                response_tx: tx,
            })
            .await
            .expect("Failed to record message in chain");
        rx.await.expect("Failed to get response from chain");
        let client = reqwest::Client::new();
        let _response = client
            .post(&address)
            .json(&msg_value)
            .send()
            .await
            .expect("Failed to send message");
    });
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct HttpRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

async fn http_send(
    chain_tx: Sender<ChainRequest>,
    address: String,
    msg: Vec<u8>,
) -> Result<(Vec<u8>,)> {
    let msg_value: Value = serde_json::from_slice(&msg).expect("Failed to parse message as JSON");
    let evt = Event {
        type_: "actor-message".to_string(),
        data: json!({
            "address": address,
            "message": msg_value,
        }),
    };

    let req: HttpRequest = serde_json::from_value(msg_value).expect("Failed to parse request");

    let response_bytes = tokio::spawn(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();

        chain_tx
            .send(ChainRequest {
                request_type: ChainRequestType::AddEvent { event: evt },
                response_tx: tx,
            })
            .await
            .expect("Failed to record message in chain");
        rx.await.expect("Failed to get response from chain");

        let client = reqwest::Client::new();
        let request = client
            .request(
                Method::from_bytes(req.method.as_bytes()).expect("Failed to parse method"),
                &address,
            )
            .headers(
                req.headers
                    .iter()
                    .map(|(name, value)| {
                        (
                            reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                            reqwest::header::HeaderValue::from_str(value).unwrap(),
                        )
                    })
                    .collect(),
            )
            .body(req.body.unwrap_or_default());

        let response = request.send().await.expect("Failed to send request");

        let status = response.status().as_u16();

        let mut headers = Vec::new();
        for (name, value) in response.headers() {
            headers.push((
                name.as_str().to_string(),
                value.to_str().unwrap().to_string(),
            ));
        }

        let body = response.bytes().await.expect("Failed to get response body");

        let response = HttpResponse {
            status,
            headers,
            body: Some(body.to_vec()),
        };

        let response_bytes = serde_json::to_vec(&response).expect("Failed to serialize response");

        let chain_tx = chain_tx.clone();
        let evt = Event {
            type_: "actor-message".to_string(),
            data: json!({
                "address": address,
                "message": response_bytes,
            }),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();

        chain_tx
            .send(ChainRequest {
                request_type: ChainRequestType::AddEvent { event: evt },
                response_tx: tx,
            })
            .await
            .expect("Failed to record message in chain");

        rx.await.expect("Failed to get response from chain");

        let response_bytes = serde_json::to_vec(&response).expect("Failed to serialize response");

        (response_bytes,)
    })
    .await?;

    Ok(response_bytes)
}
