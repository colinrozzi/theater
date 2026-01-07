//! HTTP server implementation for wasi:http/incoming-handler
//!
//! This module provides a hyper-based HTTP server that forwards requests
//! to WebAssembly components that export the wasi:http/incoming-handler interface.

use crate::incoming::{IncomingRequestResource, ResponseOutparam, ResponseOutparamResult};
use crate::types::{Headers, WasiMethod, WasiScheme};
use crate::events::HttpEventData;
use theater::handler::SharedActorInstance;
use theater::events::theater_runtime::TheaterRuntimeEventData;
use theater::events::wasm::WasmEventData;
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
    // Extract request information
    let method = convert_method(req.method());
    let scheme = Some(WasiScheme::Http);
    let authority = req.uri().authority().map(|a| a.to_string());
    let path_with_query = req.uri().path_and_query().map(|pq| pq.to_string());

    // Convert headers
    let headers = Headers::new();
    for (name, value) in req.headers() {
        headers.append(name.as_str(), value.as_bytes().to_vec());
    }

    // Read the body
    let body_bytes = req.collect().await
        .context("Failed to read request body")?
        .to_bytes()
        .to_vec();

    debug!(
        "Handling incoming request: {} {:?}",
        method_to_string(&method),
        path_with_query
    );

    // Create the incoming request resource
    let incoming_request = IncomingRequestResource::new(
        method.clone(),
        scheme,
        authority,
        path_with_query.clone(),
        headers,
        body_bytes,
    );

    // Create a channel for the response
    let (response_tx, response_rx) = oneshot::channel();
    let response_outparam = ResponseOutparam::new(response_tx);

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
        let request_resource: Resource<IncomingRequestResource> = {
            let mut table = actor_instance.actor_component.actor_store.resource_table.lock().unwrap();
            table.push(incoming_request)?
        };

        let outparam_resource: Resource<ResponseOutparam> = {
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

        // Then get the handle function
        let func = actor_instance.instance.get_func(
            &mut actor_instance.store,
            &interface_export,
        );

        let func = match func {
            Some(f) => f,
            None => {
                bail!("Component does not export 'handle' function in wasi:http/incoming-handler@0.2.0");
            }
        };

        // Call the function with the resource parameters
        let params = [Val::Resource(request_any), Val::Resource(outparam_any)];
        let mut results = [];

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

    // Convert the response to hyper format
    match response_result {
        ResponseOutparamResult::Response(response_resource) => {
            // Get the response from the resource table
            let guard = shared_instance.write().await;
            let actor_instance = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Actor instance not available"))?;

            let table = actor_instance.actor_component.actor_store.resource_table.lock().unwrap();
            let response = table.get(&response_resource)?;

            let status = response.status();
            let response_headers = response.headers().clone();

            // Get body contents
            let body_contents = {
                let body_lock = response.body.lock().unwrap();
                if let Some(body) = &*body_lock {
                    body.get_contents()
                } else {
                    Vec::new()
                }
            };

            drop(table);
            drop(guard);

            // Build the hyper response
            let mut builder = Response::builder().status(status);

            for (name, values) in response_headers.entries() {
                for value in values {
                    if let Ok(header_value) = hyper::header::HeaderValue::from_bytes(&value) {
                        builder = builder.header(&name, header_value);
                    }
                }
            }

            Ok(builder.body(Full::new(Bytes::from(body_contents)))?)
        }
        ResponseOutparamResult::Error(error_code) => {
            error!("Component returned error: {:?}", error_code);
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
