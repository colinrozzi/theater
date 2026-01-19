//! HTTP Replay Module
//!
//! This module provides functionality for replaying recorded HTTP events
//! to WebAssembly components during replay mode. Instead of starting a
//! real HTTP server, it reads recorded HTTP request/response pairs and
//! feeds them to the component's incoming-handler export.

use crate::events::{HttpRequestData, HttpResponseData};
use crate::types::{
    HostFields, HostIncomingBody, HostIncomingRequest, HostResponseOutparam, Method,
    ResponseOutparamResult, Scheme,
};
use anyhow::{bail, Context, Result};
use base64::Engine;
use theater::chain::HttpReplayChain;
use theater::handler::SharedActorInstance;
use theater::replay::HostFunctionCall;
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};
use val_serde::SerializableVal;
use wasmtime::component::{Resource, ResourceAny, Val};

/// Replay all HTTP events from the chain to the component.
///
/// This function:
/// 1. Extracts HTTP incoming handler events from the replay chain
/// 2. For each event, reconstructs the request and calls the component's handler
/// 3. Captures the response and compares it to the recorded response
/// 4. Records new events for verification
pub async fn replay_http_events(
    replay_chain: HttpReplayChain,
    actor_instance: SharedActorInstance,
) -> Result<()> {
    let http_events = replay_chain.http_incoming_events();

    if http_events.is_empty() {
        info!("No HTTP events to replay");
        return Ok(());
    }

    info!("Replaying {} HTTP events", http_events.len());

    for (idx, event) in http_events.iter().enumerate() {
        info!("Replaying HTTP event {}/{}", idx + 1, http_events.len());

        // Parse the event data to get the recorded request and response
        let host_call: HostFunctionCall = serde_json::from_slice(&event.data)
            .context("Failed to parse HTTP event data as HostFunctionCall")?;

        // Parse request and response from the host call (stored as JSON strings in SerializableVal)
        let request_json = match &host_call.input {
            SerializableVal::String(s) => s.as_str(),
            _ => bail!("Expected HTTP request data as SerializableVal::String"),
        };
        let response_json = match &host_call.output {
            SerializableVal::String(s) => s.as_str(),
            _ => bail!("Expected HTTP response data as SerializableVal::String"),
        };
        let request_data: HttpRequestData = serde_json::from_str(request_json)
            .context("Failed to parse HTTP request data")?;
        let expected_response: HttpResponseData = serde_json::from_str(response_json)
            .context("Failed to parse HTTP response data")?;

        debug!(
            "Replaying request: {} {}",
            request_data.method,
            request_data.path_with_query.as_deref().unwrap_or("/")
        );

        // Replay this request
        match replay_single_request(&request_data, &expected_response, &actor_instance).await {
            Ok(()) => {
                info!("HTTP event {}/{} replayed successfully", idx + 1, http_events.len());
            }
            Err(e) => {
                error!("HTTP event {}/{} replay failed: {:?}", idx + 1, http_events.len(), e);
                return Err(e);
            }
        }
    }

    info!("All {} HTTP events replayed successfully", http_events.len());
    Ok(())
}

/// Replay a single HTTP request to the component.
async fn replay_single_request(
    request_data: &HttpRequestData,
    expected_response: &HttpResponseData,
    shared_instance: &SharedActorInstance,
) -> Result<()> {
    let b64 = base64::engine::general_purpose::STANDARD;

    // Reconstruct the request from recorded data
    let method = Method::from_string(&request_data.method);
    let scheme = request_data
        .scheme
        .as_ref()
        .map(|s| Scheme::from_string(s));

    // Reconstruct headers
    let mut headers = HostFields::new();
    for (name, value_b64) in &request_data.headers {
        let value_bytes = b64
            .decode(value_b64)
            .context("Failed to decode header value")?;
        headers.append(name, value_bytes);
    }

    // Reconstruct body
    let body_bytes = if let Some(body_b64) = &request_data.body {
        b64.decode(body_b64).context("Failed to decode body")?
    } else {
        Vec::new()
    };

    // Create the incoming request resource
    let incoming_request = HostIncomingRequest {
        method,
        scheme,
        authority: request_data.authority.clone(),
        path_with_query: request_data.path_with_query.clone(),
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

    // Call the component's handler
    {
        let mut guard = shared_instance.write().await;
        let unified_instance = match &mut *guard {
            Some(instance) => instance,
            None => {
                bail!("Actor instance not available for replay");
            }
        };

        // HTTP replay handler only supports wasmtime instances
        let actor_instance = unified_instance
            .as_wasmtime_mut()
            .ok_or_else(|| anyhow::anyhow!("HTTP replay handler does not support Composite instances"))?;

        // Push resources to the resource table
        let request_resource: Resource<HostIncomingRequest> = {
            let mut table = actor_instance
                .actor_component
                .actor_store
                .resource_table
                .lock()
                .unwrap();
            table.push(incoming_request)?
        };

        let outparam_resource: Resource<HostResponseOutparam> = {
            let mut table = actor_instance
                .actor_component
                .actor_store
                .resource_table
                .lock()
                .unwrap();
            table.push(response_outparam)?
        };

        // Convert to ResourceAny
        let request_any =
            ResourceAny::try_from_resource(request_resource, &mut actor_instance.store)?;
        let outparam_any =
            ResourceAny::try_from_resource(outparam_resource, &mut actor_instance.store)?;

        // Get the exported handle function
        let interface_export = actor_instance
            .instance
            .get_export(
                &mut actor_instance.store,
                None,
                "wasi:http/incoming-handler@0.2.0",
            )
            .context("Component does not export wasi:http/incoming-handler@0.2.0")?;

        let handle_export = actor_instance
            .instance
            .get_export(
                &mut actor_instance.store,
                Some(&interface_export),
                "handle",
            )
            .context("Component does not export 'handle' function")?;

        let func = actor_instance
            .instance
            .get_func(&mut actor_instance.store, &handle_export)
            .context("Failed to get Func for 'handle'")?;

        // Call the function
        let params = [Val::Resource(request_any), Val::Resource(outparam_any)];
        let mut results = [];

        func.call_async(&mut actor_instance.store, &params, &mut results)
            .await
            .context("Failed to call wasi:http/incoming-handler.handle")?;

        func.post_return_async(&mut actor_instance.store)
            .await
            .context("Failed to post_return")?;
    }

    // Wait for the response
    let response_result = response_rx
        .await
        .context("Response channel closed without response")?;

    // Verify the response matches expected
    match response_result {
        ResponseOutparamResult::Response(response) => {
            let actual_status = response.status;
            let actual_body = if let Some(body) = &response.body {
                if let Some(stream) = &body.stream {
                    stream.get_contents()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // Compare status codes
            if actual_status != expected_response.status_code {
                warn!(
                    "Response status mismatch: expected {}, got {}",
                    expected_response.status_code, actual_status
                );
            }

            // Compare body (decode expected body from base64)
            let expected_body = b64
                .decode(&expected_response.body)
                .unwrap_or_else(|_| Vec::new());

            if actual_body != expected_body {
                warn!(
                    "Response body mismatch: expected {} bytes, got {} bytes",
                    expected_body.len(),
                    actual_body.len()
                );
                debug!("Expected body: {:?}", String::from_utf8_lossy(&expected_body));
                debug!("Actual body: {:?}", String::from_utf8_lossy(&actual_body));
            }

            // Record the replay event
            {
                let guard = shared_instance.read().await;
                if let Some(unified_instance) = &*guard {
                    if let Some(instance) = unified_instance.as_wasmtime() {
                        let response_headers: Vec<(String, String)> = response
                            .headers
                            .entries()
                            .iter()
                            .map(|(name, value)| (name.clone(), b64.encode(value)))
                            .collect();

                        let actual_response_data = HttpResponseData {
                            status_code: actual_status,
                            headers: response_headers,
                            body: b64.encode(&actual_body),
                        };

                        // Serialize structs to JSON strings for recording
                        let request_json = serde_json::to_string(&request_data).unwrap_or_default();
                        let response_json = serde_json::to_string(&actual_response_data).unwrap_or_default();
                        instance
                            .actor_component
                            .actor_store
                            .record_host_function_call(
                                "wasi:http/incoming-handler@0.2.0",
                                "handle",
                                SerializableVal::String(request_json),
                                SerializableVal::String(response_json),
                            );
                    }
                }
            }

            Ok(())
        }
        ResponseOutparamResult::Error(error_code) => {
            error!("Component returned error during replay: {:?}", error_code);
            bail!("Component returned error: {:?}", error_code);
        }
    }
}
