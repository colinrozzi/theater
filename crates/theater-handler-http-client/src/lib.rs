//! # HTTP Client Handler
//!
//! Provides a single outbound HTTP(S) request primitive (`request`) to
//! WebAssembly actors in the Theater system. Built in the modern packr /
//! Graph-ABI style — it embeds `http-client.pact`, marshals the request /
//! response records to and from packr `Value`s, and makes the call with
//! `reqwest` (rustls, no OpenSSL).
//!
//! ## Permission model
//!
//! Every request is permission-gated. The handler is configured with an
//! `allowed_hosts` allowlist in the actor's manifest:
//!
//! ```toml
//! [[handler]]
//! type = "http-client"
//! allowed_hosts = ["api.example.com", "example.org"]
//! ```
//!
//! A request whose URL host is not in `allowed_hosts` is rejected before any
//! network I/O happens, returning `Err("http-client: host '<h>' not in
//! allowed_hosts")`. An empty allowlist rejects every host.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tracing::{debug, error, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::{HandlerConfig, HttpClientHandlerConfig};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
};

// ============================================================================
// Interface
// ============================================================================

const HTTP_CLIENT_PACT: &str = include_str!("../http-client.pact");

fn http_client_interface() -> InterfaceImpl {
    let pact = parse_pact(HTTP_CLIENT_PACT).expect("embedded http-client.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Handler
// ============================================================================

/// Handler that lets actors make outbound HTTP(S) requests, gated by a
/// per-manifest allowed-hosts allowlist.
#[derive(Clone)]
pub struct HttpClientHandler {
    config: HttpClientHandlerConfig,
    /// Shared reqwest client (rustls). `reqwest::Client` is cheap to clone —
    /// it wraps an internal `Arc` over the connection pool.
    client: reqwest::Client,
}

impl HttpClientHandler {
    pub fn new(config: HttpClientHandlerConfig) -> Self {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .unwrap_or_else(|e| {
                error!("http-client: failed to build rustls client: {e}; using default client");
                reqwest::Client::new()
            });
        Self { config, client }
    }
}

impl Handler for HttpClientHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let cfg = match config {
            Some(HandlerConfig::HttpClient { config }) => config.clone(),
            _ => self.config.clone(),
        };
        Box::new(HttpClientHandler::new(cfg))
    }

    fn name(&self) -> &str {
        "http-client"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(
            self.interfaces()
                .iter()
                .map(|i| i.name().to_string())
                .collect(),
        )
    }

    fn exports(&self) -> Option<Vec<String>> {
        // Import-only handler: the guest imports theater:simple/http-client and
        // calls request(); there is no guest-exported callback interface.
        None
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![http_client_interface()]
    }

    fn setup(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("HTTP client handler setup (passive)");
        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("HTTP client handler shutting down");
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up HTTP client host functions");

        if ctx.is_satisfied("theater:simple/http-client") {
            info!("theater:simple/http-client already satisfied, skipping");
            return Ok(());
        }

        let client = self.client.clone();
        let allowed_hosts = Arc::new(self.config.allowed_hosts.clone());

        builder
            .interface("theater:simple/http-client")?
            // request(req: http-request) -> result<http-response, string>
            .func_async_result(
                "request",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let client = client.clone();
                    let allowed = allowed_hosts.clone();
                    async move {
                        let parts = parse_http_request(&input)?;

                        // Permission check: the URL host must be allowlisted.
                        let url = reqwest::Url::parse(&parts.url).map_err(|e| {
                            Value::String(format!(
                                "http-client: invalid url '{}': {}",
                                parts.url, e
                            ))
                        })?;
                        let host = url.host_str().ok_or_else(|| {
                            Value::String(format!("http-client: url '{}' has no host", parts.url))
                        })?;
                        if !allowed.iter().any(|h| h == host) {
                            return Err(Value::String(format!(
                                "http-client: host '{}' not in allowed_hosts",
                                host
                            )));
                        }

                        debug!("http-client: {} {}", parts.method, parts.url);
                        do_request(&client, parts).await
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/http-client");
        Ok(())
    }
}

// ============================================================================
// Request parsing (Value -> parts)
// ============================================================================

struct HttpRequestParts {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

/// Parse the incoming `http-request` record `Value` into owned parts. Records
/// marshal as `Value::Record { fields }`; a `Value::Tuple` (positional
/// method/url/headers/body) is accepted defensively.
fn parse_http_request(input: &Value) -> Result<HttpRequestParts, Value> {
    let mut parts = HttpRequestParts {
        method: String::new(),
        url: String::new(),
        headers: Vec::new(),
        body: None,
    };

    match input {
        Value::Record { fields, .. } => {
            for (key, val) in fields {
                match (key.as_str(), val) {
                    ("method", Value::String(s)) => parts.method = s.clone(),
                    ("url", Value::String(s)) => parts.url = s.clone(),
                    ("headers", v) => parts.headers = parse_headers(v),
                    ("body", v) => parts.body = parse_body(v),
                    _ => {}
                }
            }
        }
        Value::Tuple(items) if items.len() >= 2 => {
            if let Value::String(s) = &items[0] {
                parts.method = s.clone();
            }
            if let Value::String(s) = &items[1] {
                parts.url = s.clone();
            }
            if let Some(v) = items.get(2) {
                parts.headers = parse_headers(v);
            }
            if let Some(v) = items.get(3) {
                parts.body = parse_body(v);
            }
        }
        _ => {
            return Err(Value::String(
                "http-client: expected http-request record".to_string(),
            ))
        }
    }

    if parts.method.is_empty() {
        return Err(Value::String(
            "http-client: http-request.method is required".to_string(),
        ));
    }
    if parts.url.is_empty() {
        return Err(Value::String(
            "http-client: http-request.url is required".to_string(),
        ));
    }
    Ok(parts)
}

/// Parse a `list<http-header>` `Value` into name/value pairs. Each entry is an
/// `http-header` record; a `(string, string)` tuple is accepted defensively.
fn parse_headers(v: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Value::List { items, .. } = v {
        for item in items {
            match item {
                Value::Record { fields, .. } => {
                    let mut name = String::new();
                    let mut value = String::new();
                    for (key, val) in fields {
                        match (key.as_str(), val) {
                            ("name", Value::String(s)) => name = s.clone(),
                            ("value", Value::String(s)) => value = s.clone(),
                            _ => {}
                        }
                    }
                    out.push((name, value));
                }
                Value::Tuple(t) if t.len() >= 2 => {
                    if let (Value::String(n), Value::String(val)) = (&t[0], &t[1]) {
                        out.push((n.clone(), val.clone()));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Parse an `option<list<u8>>` body `Value`. A bare `list<u8>` is accepted
/// defensively as a present body.
fn parse_body(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Option {
            value: Some(inner), ..
        } => bytes_from_list(inner),
        Value::Option { value: None, .. } => None,
        Value::List { .. } => bytes_from_list(v),
        _ => None,
    }
}

fn bytes_from_list(v: &Value) -> Option<Vec<u8>> {
    if let Value::List { items, .. } = v {
        Some(
            items
                .iter()
                .filter_map(|x| if let Value::U8(b) = x { Some(*b) } else { None })
                .collect(),
        )
    } else {
        None
    }
}

// ============================================================================
// The request itself (parts -> Value)
// ============================================================================

async fn do_request(client: &reqwest::Client, parts: HttpRequestParts) -> Result<Value, Value> {
    let method = reqwest::Method::from_bytes(parts.method.as_bytes()).map_err(|e| {
        Value::String(format!(
            "http-client: invalid method '{}': {}",
            parts.method, e
        ))
    })?;

    let mut req = client.request(method, parts.url);
    for (name, value) in &parts.headers {
        let header_name =
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
                Value::String(format!(
                    "http-client: invalid header name '{}': {}",
                    name, e
                ))
            })?;
        let header_value = reqwest::header::HeaderValue::from_str(value).map_err(|e| {
            Value::String(format!(
                "http-client: invalid header value for '{}': {}",
                name, e
            ))
        })?;
        req = req.header(header_name, header_value);
    }
    if let Some(body) = parts.body {
        req = req.body(body);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| Value::String(format!("http-client: request failed: {}", e)))?;

    let status = resp.status().as_u16();

    let headers: Vec<Value> = resp
        .headers()
        .iter()
        .map(|(name, value)| Value::Record {
            type_name: "http-header".to_string(),
            fields: vec![
                ("name".to_string(), Value::String(name.as_str().to_string())),
                (
                    "value".to_string(),
                    Value::String(String::from_utf8_lossy(value.as_bytes()).into_owned()),
                ),
            ],
        })
        .collect();

    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| Value::String(format!("http-client: reading response body failed: {}", e)))?;

    let body_list = Value::List {
        elem_type: ValueType::U8,
        items: body_bytes.iter().map(|b| Value::U8(*b)).collect(),
    };
    let body = Value::Option {
        inner_type: body_list.infer_type(),
        value: Some(Box::new(body_list)),
    };

    Ok(Value::Record {
        type_name: "http-response".to_string(),
        fields: vec![
            ("status".to_string(), Value::U16(status)),
            (
                "headers".to_string(),
                Value::List {
                    elem_type: ValueType::Record("http-header".to_string()),
                    items: headers,
                },
            ),
            ("body".to_string(), body),
        ],
    })
}
