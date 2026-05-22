//! # Podman Handler
//!
//! Provides container management (run / stop / rm / list) to WebAssembly
//! actors in the Theater system. Shells out to the `podman` CLI — no daemon
//! required.
//!
//! The substrate for orchestrating agent containers from inside a Theater
//! actor (agentry et al).

use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use tokio::process::Command;
use tracing::{debug, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::{HandlerConfig, PodmanHandlerConfig};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
};

// ============================================================================
// Interface
// ============================================================================

const PODMAN_PACT: &str = include_str!("../podman.pact");

fn podman_interface() -> InterfaceImpl {
    let pact = parse_pact(PODMAN_PACT).expect("embedded podman.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Handler
// ============================================================================

#[derive(Clone)]
pub struct PodmanHandler {
    config: PodmanHandlerConfig,
}

impl PodmanHandler {
    pub fn new(config: PodmanHandlerConfig) -> Self {
        Self { config }
    }
}

impl Handler for PodmanHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let cfg = match config {
            Some(HandlerConfig::Podman { config }) => config.clone(),
            _ => self.config.clone(),
        };
        Box::new(PodmanHandler::new(cfg))
    }

    fn name(&self) -> &str {
        "podman"
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
        Some(vec!["theater:simple/podman".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![podman_interface()]
    }

    fn setup(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: tokio::sync::broadcast::Receiver<theater::chain::ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Podman handler setup (passive)");
        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("Podman handler shutting down");
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up podman host functions");

        if ctx.is_satisfied("theater:simple/podman") {
            info!("theater:simple/podman already satisfied, skipping");
            return Ok(());
        }

        builder
            .interface("theater:simple/podman")?
            // run(spec: container-spec) -> result<string, string>
            .func_async_result(
                "run",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| async move {
                    let spec = parse_container_spec(&input)?;
                    debug!("podman.run name={} image={}", spec.name, spec.image);
                    run_container(spec)
                        .await
                        .map(Value::String)
                        .map_err(Value::String)
                },
            )?
            // stop(name: string) -> result<_, string>
            .func_async_result(
                "stop",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| async move {
                    let name = parse_string(&input)?;
                    debug!("podman.stop name={}", name);
                    stop_container(&name)
                        .await
                        .map(|_| Value::Tuple(Vec::new()))
                        .map_err(Value::String)
                },
            )?
            // rm(name: string, force: bool) -> result<_, string>
            .func_async_result(
                "rm",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| async move {
                    let (name, force) = parse_rm_args(&input)?;
                    debug!("podman.rm name={} force={}", name, force);
                    rm_container(&name, force)
                        .await
                        .map(|_| Value::Tuple(Vec::new()))
                        .map_err(Value::String)
                },
            )?
            // list() -> result<list<container-info>, string>
            .func_async_result(
                "list",
                move |_ctx: AsyncCtx<ActorStore>, _input: Value| async move {
                    list_containers()
                        .await
                        .map(|items| Value::List {
                            elem_type: ValueType::Record("container-info".to_string()),
                            items,
                        })
                        .map_err(Value::String)
                },
            )?;

        ctx.mark_satisfied("theater:simple/podman");
        Ok(())
    }
}

// ============================================================================
// Input parsing
// ============================================================================

fn parse_string(input: &Value) -> Result<String, Value> {
    match input {
        Value::String(s) => Ok(s.clone()),
        _ => Err(Value::String("expected string".to_string())),
    }
}

fn parse_rm_args(input: &Value) -> Result<(String, bool), Value> {
    match input {
        Value::Tuple(items) if items.len() >= 2 => {
            let name = match &items[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("rm: expected string for name".to_string())),
            };
            let force = match &items[1] {
                Value::Bool(b) => *b,
                _ => return Err(Value::String("rm: expected bool for force".to_string())),
            };
            Ok((name, force))
        }
        _ => Err(Value::String("rm: expected (string, bool)".to_string())),
    }
}

#[derive(Debug, Clone)]
struct MountSpec {
    source: String,
    target: String,
    read_only: bool,
}

#[derive(Debug, Clone)]
struct ContainerSpec {
    image: String,
    name: String,
    env: Vec<(String, String)>,
    mounts: Vec<MountSpec>,
    /// Empty = use the image's default command.
    cmd: Vec<String>,
    tty: bool,
    interactive: bool,
}

fn parse_container_spec(input: &Value) -> Result<ContainerSpec, Value> {
    let fields = match input {
        Value::Record { fields, .. } => fields,
        _ => return Err(Value::String("expected container-spec record".to_string())),
    };
    let mut spec = ContainerSpec {
        image: String::new(),
        name: String::new(),
        env: Vec::new(),
        mounts: Vec::new(),
        cmd: Vec::new(),
        tty: false,
        interactive: false,
    };
    for (key, val) in fields {
        match (key.as_str(), val) {
            ("image", Value::String(s)) => spec.image = s.clone(),
            ("name", Value::String(s)) => spec.name = s.clone(),
            ("env", Value::List { items, .. }) => {
                for item in items {
                    if let Value::Tuple(t) = item {
                        if t.len() >= 2 {
                            if let (Value::String(k), Value::String(v)) = (&t[0], &t[1]) {
                                spec.env.push((k.clone(), v.clone()));
                            }
                        }
                    }
                }
            }
            ("mounts", Value::List { items, .. }) => {
                for item in items {
                    if let Value::Record { fields, .. } = item {
                        let mut m = MountSpec {
                            source: String::new(),
                            target: String::new(),
                            read_only: false,
                        };
                        for (mk, mv) in fields {
                            match (mk.as_str(), mv) {
                                ("source", Value::String(s)) => m.source = s.clone(),
                                ("target", Value::String(s)) => m.target = s.clone(),
                                ("read-only", Value::Bool(b)) => m.read_only = *b,
                                _ => {}
                            }
                        }
                        spec.mounts.push(m);
                    }
                }
            }
            ("cmd", Value::List { items, .. }) => {
                spec.cmd = items
                    .iter()
                    .filter_map(|i| {
                        if let Value::String(s) = i {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
            }
            ("tty", Value::Bool(b)) => spec.tty = *b,
            ("interactive", Value::Bool(b)) => spec.interactive = *b,
            _ => {}
        }
    }
    if spec.image.is_empty() {
        return Err(Value::String(
            "container-spec.image is required".to_string(),
        ));
    }
    if spec.name.is_empty() {
        return Err(Value::String("container-spec.name is required".to_string()));
    }
    Ok(spec)
}

// ============================================================================
// Podman shell-outs
// ============================================================================

async fn run_container(spec: ContainerSpec) -> Result<String, String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--name".into(),
        spec.name.clone(),
    ];
    if spec.tty {
        args.push("-t".into());
    }
    if spec.interactive {
        args.push("-i".into());
    }
    for (k, v) in &spec.env {
        args.push("--env".into());
        args.push(format!("{}={}", k, v));
    }
    for m in &spec.mounts {
        args.push("-v".into());
        let mode = if m.read_only { ":ro" } else { "" };
        args.push(format!("{}:{}{}", m.source, m.target, mode));
    }
    args.push(spec.image.clone());
    // Empty cmd = use image default.
    args.extend(spec.cmd.iter().cloned());

    let output = Command::new("podman")
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("spawn podman: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "podman run failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn stop_container(name: &str) -> Result<(), String> {
    let output = Command::new("podman")
        .args(["stop", "--time", "10", name])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("spawn podman: {}", e))?;
    if output.status.success() {
        return Ok(());
    }
    // Treat "no such container" and "already stopped" as success — caller
    // wants idempotent semantics.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("no such container") || stderr.contains("is not running") {
        return Ok(());
    }
    Err(format!("podman stop failed: {}", stderr.trim()))
}

async fn rm_container(name: &str, force: bool) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["rm"];
    if force {
        args.push("-f");
    }
    args.push(name);
    let output = Command::new("podman")
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("spawn podman: {}", e))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("no such container") {
        return Ok(());
    }
    Err(format!("podman rm failed: {}", stderr.trim()))
}

#[derive(serde::Deserialize)]
struct PodmanPsEntry {
    #[serde(rename = "Id", default)]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
    #[serde(rename = "Image", default)]
    image: String,
    #[serde(rename = "State", default)]
    state: String,
    #[serde(rename = "ExitCode", default)]
    exit_code: Option<i32>,
}

async fn list_containers() -> Result<Vec<Value>, String> {
    let output = Command::new("podman")
        .args(["ps", "-a", "--format", "json"])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("spawn podman: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "podman ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    // podman ps --format json returns a JSON array (possibly empty, "[]")
    let entries: Vec<PodmanPsEntry> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("parse podman ps json: {}", e))?;
    let infos = entries
        .into_iter()
        .map(|e| {
            let name = e.names.into_iter().next().unwrap_or_default();
            // -1 = still running (no exit code yet). Otherwise report the
            // recorded exit code, falling back to 0 if podman didn't supply
            // one (shouldn't happen for exited containers, but be safe).
            let exit_code = if e.state == "exited" || e.state == "stopped" {
                e.exit_code.unwrap_or(0)
            } else {
                -1
            };
            build_container_info(&e.id, &name, &e.image, &e.state, exit_code)
        })
        .collect();
    Ok(infos)
}

fn build_container_info(id: &str, name: &str, image: &str, status: &str, exit_code: i32) -> Value {
    Value::Record {
        type_name: "container-info".to_string(),
        fields: vec![
            ("id".to_string(), Value::String(id.to_string())),
            ("name".to_string(), Value::String(name.to_string())),
            ("image".to_string(), Value::String(image.to_string())),
            ("status".to_string(), Value::String(status.to_string())),
            ("exit-code".to_string(), Value::S32(exit_code)),
        ],
    }
}
