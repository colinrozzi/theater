//! HTTP server implementation for wasi:http/incoming-handler
//!
//! This module provides a hyper-based HTTP server that forwards requests
//! to WebAssembly components that export the wasi:http/incoming-handler interface.

use crate::types::{
    HostIncomingRequest, HostResponseOutparam, HostIncomingBody, HostFields,
    HostOutgoingResponse, Method, Scheme, ResponseOutparamResult, WasiMethod, WasiScheme
};
use crate::events::{HttpRequestData, HttpResponseData};
use base64::Engine;
use theater::events::{ChainEventData, ChainEventPayload, wasm::WasmEventData};
use theater::handler::SharedActorInstance;
use theater::shutdown::ShutdownReceiver;
use anyhow::{Result, bail, Context};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::{debug, error, info};
use wasmtime::component::{Resource, ResourceAny, Val};

/// Start the incoming HTTP server
pub async fn start_incoming_server(
    actor_instance: SharedActorInstance,
    host: &str,
    port: u16,
    shutdown_receiver: ShutdownReceiver,
) -> Result<()>
{
    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    let listener = TcpListener::bind(addr).await?;

    info!("WASI HTTP server listening on {}", addr);

    // Wrap the actor instance in Arc for sharing across connections
    let shared_instance = Arc::new(actor_instance);

    // Convert shutdown receiver to a future we can select on
    let mut shutdown_rx = shutdown_receiver.receiver;

    loop {
        tokio::select! {
            // Handle incoming connections
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, remote_addr)) => {
                        debug!("Accepted connection from {}", remote_addr);
                        let shared_instance = shared_instance.clone();

                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                                let shared_instance = shared_instance.clone();
                                async move {
                                    handle_request(req, shared_instance).await
                                }
                            });

                            if let Err(e) = http1::Builder::new()
                                .serve_connection(io, service)
                                .await
                            {
                                error!("Error serving connection from {}: {:?}", remote_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Error accepting connection: {:?}", e);
                    }
                }
            }
            // Handle shutdown signal
            _ = &mut shutdown_rx => {
                info!("Received shutdown signal, stopping HTTP server");
                break;
            }
        }
    }

    Ok(())
}

/// Handle a single HTTP request by calling the component's incoming-handler.handle export
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    actor_instance: Arc<SharedActorInstance>,
) -> Result<Response<Full<Bytes>>, Infallible>
{
    match handle_request_inner(req, actor_instance).await {
        Ok(response) => Ok(response),
        Err(e) => {
            error!("Error handling HTTP request: {:?}", e);
            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from(format!("Internal Server Error: {}", e))))
                .unwrap())
        }
    }
}

async fn handle_request_inner(
    req: Request<hyper::body::Incoming>,
    shared_instance: Arc<SharedActorInstance>,
) -> Result<Response<Full<Bytes>>>
{
    info!("handle_request_inner called for path: {:?}", req.uri().path());
    let b64 = base64::engine::general_purpose::STANDARD;

    // Extract request information
    let wasi_method = convert_method(req.method());
    let method = Method::from_wasi(&wasi_method);
    let wasi_scheme = WasiScheme::Http;
    let scheme = Some(Scheme::from_wasi(&wasi_scheme));
    let authority = req.uri().authority().map(|a| a.to_string());
    let path_with_query = req.uri().path_and_query().map(|pq| pq.to_string());

    // Convert headers using HostFields (the backing type for WASI)
    // Also capture for event recording
    let mut headers = HostFields::new();
    let mut request_headers_for_event: Vec<(String, String)> = Vec::new();
    for (name, value) in req.headers() {
        let value_bytes = value.as_bytes().to_vec();
        headers.append(name.as_str(), value_bytes.clone());
        request_headers_for_event.push((name.to_string(), b64.encode(&value_bytes)));
    }

    // Read the body
    let body_bytes = req.collect().await
        .context("Failed to read request body")?
        .to_bytes()
        .to_vec();

    // Capture request data for event recording
    let request_event_data = HttpRequestData {
        method: method.as_str().to_string(),
        scheme: Some("http".to_string()),
        authority: authority.clone(),
        path_with_query: path_with_query.clone(),
        headers: request_headers_for_event,
        body: if body_bytes.is_empty() {
            None
        } else {
            Some(b64.encode(&body_bytes))
        },
    };

    debug!(
        "Handling incoming request: {} {:?}",
        method.as_str(),
        path_with_query
    );

    // Create the incoming request resource using HostIncomingRequest
    let incoming_request = HostIncomingRequest {
        method,
        scheme,
        authority,
        path_with_query: path_with_query.clone(),
        headers,
        body: if body_bytes.is_empty() {
            None
        } else {
            Some(HostIncomingBody::new(body_bytes))
        },
    };

    // Create a channel for the response
    let (response_tx, response_rx) = oneshot::channel();
    let response_outparam = HostResponseOutparam {
        sender: Some(response_tx),
    };

    // Get write lock on actor instance and call the handler
    {
        let mut guard = shared_instance.write().await;
        let actor_instance = match &mut *guard {
            Some(instance) => instance,
            None => {
                bail!("Actor instance not available");
            }
        };

        // Push resources to the resource table and get Resource handles
        let request_resource: Resource<HostIncomingRequest> = {
            let mut table = actor_instance.actor_component.actor_store.resource_table.lock().unwrap();
            table.push(incoming_request)?
        };

        let outparam_resource: Resource<HostResponseOutparam> = {
            let mut table = actor_instance.actor_component.actor_store.resource_table.lock().unwrap();
            table.push(response_outparam)?
        };

        // Convert to ResourceAny for calling the function
        let request_any = ResourceAny::try_from_resource(request_resource, &mut actor_instance.store)?;
        let outparam_any = ResourceAny::try_from_resource(outparam_resource, &mut actor_instance.store)?;

        // Get the exported handle function
        // First, get the interface export index
        let interface_export = actor_instance.instance.get_export(
            &mut actor_instance.store,
            None,
            "wasi:http/incoming-handler@0.2.0",
        );

        let interface_export = match interface_export {
            Some(idx) => idx,
            None => {
                bail!("Component does not export wasi:http/incoming-handler@0.2.0");
            }
        };

        // Then get the handle function export from within the interface
        let handle_export = actor_instance.instance.get_export(
            &mut actor_instance.store,
            Some(&interface_export),
            "handle",
        );

        let handle_export = match handle_export {
            Some(idx) => idx,
            None => {
                bail!("Component does not export 'handle' function in wasi:http/incoming-handler@0.2.0");
            }
        };

        // Get the Func from the handle export
        let func = actor_instance.instance.get_func(
            &mut actor_instance.store,
            &handle_export,
        );

        let func = match func {
            Some(f) => f,
            None => {
                bail!("Failed to get Func for 'handle' in wasi:http/incoming-handler@0.2.0");
            }
        };

        // Call the function with the resource parameters
        let params = [Val::Resource(request_any), Val::Resource(outparam_any)];
        let mut results = [];

        // Record WasmCall BEFORE calling the export (like init does)
        actor_instance.actor_component.actor_store.record_event(ChainEventData {
            event_type: "wasm".to_string(),
            data: ChainEventPayload::Wasm(WasmEventData::WasmCall {
                function_name: "wasi:http/incoming-handler@0.2.0/handle".to_string(),
                params: serde_json::to_vec(&request_event_data).unwrap_or_default(),
            }),
        });

        func.call_async(&mut actor_instance.store, &params, &mut results).await
            .context("Failed to call wasi:http/incoming-handler.handle")?;

        // Post-return cleanup
        func.post_return_async(&mut actor_instance.store).await
            .context("Failed to post_return")?;
    }
    // Guard is now dropped, lock released

    // Wait for the response from the component
    let response_result = response_rx.await
        .context("Response channel closed without response")?;

    // Convert the response to hyper format and record the event
    match response_result {
        ResponseOutparamResult::Response(response) => {
            // Response is directly a HostOutgoingResponse
            let status = response.status;
            let response_headers = response.headers;

            // Get body contents from the HostOutgoingBody if present
            let body_contents = if let Some(body) = &response.body {
                if let Some(stream) = &body.stream {
                    stream.get_contents()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // Capture response data for event recording
            let response_headers_for_event: Vec<(String, String)> = response_headers
                .entries()
                .iter()
                .map(|(name, value)| (name.clone(), b64.encode(value)))
                .collect();

            let response_event_data = HttpResponseData {
                status_code: status,
                headers: response_headers_for_event,
                body: b64.encode(&body_contents),
            };

            // Record WasmResult AFTER the export completes (like init does)
            {
                info!("Attempting to record HTTP WasmResult event...");
                let guard = shared_instance.read().await;
                info!("Got read lock on shared_instance");
                if let Some(instance) = &*guard {
                    info!("Found actor instance, recording WasmResult event");
                    instance.actor_component.actor_store.record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        data: ChainEventPayload::Wasm(WasmEventData::WasmResult {
                            function_name: "wasi:http/incoming-handler@0.2.0/handle".to_string(),
                            result: (
                                Some(serde_json::to_vec(&request_event_data).unwrap_or_default()),
                                serde_json::to_vec(&response_event_data).unwrap_or_default(),
                            ),
                        }),
                    });
                    info!("Recorded HTTP WasmResult event");
                } else {
                    info!("Actor instance was None, could not record event");
                }
            }

            // Build the hyper response
            let mut builder = Response::builder().status(status);

            for (name, value) in response_headers.entries() {
                if let Ok(header_value) = hyper::header::HeaderValue::from_bytes(&value) {
                    builder = builder.header(&name, header_value);
                }
            }

            Ok(builder.body(Full::new(Bytes::from(body_contents)))?)
        }
        ResponseOutparamResult::Error(error_code) => {
            error!("Component returned error: {:?}", error_code);

            // Record error response event
            let response_event_data = HttpResponseData {
                status_code: 500,
                headers: vec![],
                body: b64.encode(format!("Error: {:?}", error_code).as_bytes()),
            };

            // Record WasmResult for error case
            {
                let guard = shared_instance.read().await;
                if let Some(instance) = &*guard {
                    instance.actor_component.actor_store.record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        data: ChainEventPayload::Wasm(WasmEventData::WasmResult {
                            function_name: "wasi:http/incoming-handler@0.2.0/handle".to_string(),
                            result: (
                                Some(serde_json::to_vec(&request_event_data).unwrap_or_default()),
                                serde_json::to_vec(&response_event_data).unwrap_or_default(),
                            ),
                        }),
                    });
                }
            }

            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from(format!("Error: {:?}", error_code))))?)
        }
    }
}

fn convert_method(method: &hyper::Method) -> WasiMethod {
    match *method {
        hyper::Method::GET => WasiMethod::Get,
        hyper::Method::HEAD => WasiMethod::Head,
        hyper::Method::POST => WasiMethod::Post,
        hyper::Method::PUT => WasiMethod::Put,
        hyper::Method::DELETE => WasiMethod::Delete,
        hyper::Method::CONNECT => WasiMethod::Connect,
        hyper::Method::OPTIONS => WasiMethod::Options,
        hyper::Method::TRACE => WasiMethod::Trace,
        hyper::Method::PATCH => WasiMethod::Patch,
        _ => WasiMethod::Other(method.to_string()),
    }
}

fn method_to_string(method: &WasiMethod) -> String {
    match method {
        WasiMethod::Get => "GET".to_string(),
        WasiMethod::Head => "HEAD".to_string(),
        WasiMethod::Post => "POST".to_string(),
        WasiMethod::Put => "PUT".to_string(),
        WasiMethod::Delete => "DELETE".to_string(),
        WasiMethod::Connect => "CONNECT".to_string(),
        WasiMethod::Options => "OPTIONS".to_string(),
        WasiMethod::Trace => "TRACE".to_string(),
        WasiMethod::Patch => "PATCH".to_string(),
        WasiMethod::Other(s) => s.clone(),
    }
}
