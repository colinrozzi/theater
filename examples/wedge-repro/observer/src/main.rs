//! Phase 2 in-process observability harness for the wedge reproduction.
//!
//! Embeds the theater runtime in this binary's tokio runtime, spawns the
//! wedge-repro supervisor manifest, and samples the actual saturation signal
//! — `theater_tx` queue depth — every ~250 ms. Direct in-process access
//! beats /proc + log-tail proxies: we measure what theater-dev's diagnosis
//! actually names, not a downstream symptom.
//!
//! Each TSV row:
//!
//!   elapsed_ms  rss_kb  ch_depth  ch_cap  warn_total  warn_delta  alive
//!
//! - `ch_depth` / `ch_cap` — current queued `TheaterCommand` count and
//!   remaining slot count on the bounded `theater_tx` mpsc. This is the
//!   channel that fills under the supervisor → child amplification.
//! - `warn_total` / `warn_delta` — running count of "Failed to send event
//!   notification" tracing events, captured by a custom `tracing` layer
//!   (no log file, no grep).
//! - `rss_kb` — own process RSS from /proc/self/status.
//! - `alive` — flips to 0 when the supervisor actor sends a final result
//!   via its `supervisor_tx` notification channel.
//!
//! Trailing comment row records exit reason + final totals.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::io::Write;

use anyhow::{anyhow, bail, Context, Result};
use tokio::sync::{mpsc, oneshot};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context as LayerContext, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

use theater::config::actor_manifest::{ManifestConfig, RuntimeHostConfig, SupervisorHostConfig};
use theater::handler::HandlerRegistry;
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::Value;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;

use theater_handler_runtime::RuntimeHandler;
use theater_handler_supervisor::SupervisorHandler;

const SAMPLE_INTERVAL_MS: u64 = 250;
const DEFAULT_TIMEOUT_SEC: u64 = 60;
/// Bounded capacity of the runtime command channel. Matches the value used
/// in `crates/theater-tests/examples/full-runtime.rs` and small enough that
/// the wedge-repro burst saturates it visibly.
const CHANNEL_CAPACITY: usize = 32;
const WARN_NEEDLE: &str = "Failed to send event notification";

struct Args {
    manifest: PathBuf,
    output: Option<PathBuf>,
    timeout_sec: u64,
    rust_log: String,
}

fn print_usage(prog: &str) {
    eprintln!("usage: {} [--manifest PATH] [--output PATH] [--timeout SEC] [--rust-log SPEC]", prog);
    eprintln!();
    eprintln!("Defaults:");
    eprintln!("  --manifest  supervisor/manifest.toml");
    eprintln!("  --output    stdout");
    eprintln!("  --timeout   {} seconds", DEFAULT_TIMEOUT_SEC);
    eprintln!("  --rust-log  theater=info,theater_handler_supervisor=debug");
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args();
    let prog = args.next().unwrap_or_else(|| "wedge-observe".to_string());
    let mut manifest = PathBuf::from("supervisor/manifest.toml");
    let mut output: Option<PathBuf> = None;
    let mut timeout_sec = DEFAULT_TIMEOUT_SEC;
    let mut rust_log = String::from("theater=info,theater_handler_supervisor=debug");

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => manifest = args.next().ok_or("--manifest needs PATH")?.into(),
            "--output" => output = Some(args.next().ok_or("--output needs PATH")?.into()),
            "--timeout" => {
                timeout_sec = args.next().ok_or("--timeout needs SEC")?
                    .parse().map_err(|e| format!("--timeout: {}", e))?;
            }
            "--rust-log" => rust_log = args.next().ok_or("--rust-log needs SPEC")?,
            "-h" | "--help" => { print_usage(&prog); std::process::exit(0); }
            other => return Err(format!("unknown arg: {}", other)),
        }
    }
    Ok(Args { manifest, output, timeout_sec, rust_log })
}

/// Counts tracing events whose formatted message contains the wedge needle.
struct WarnCounter {
    count: Arc<AtomicU64>,
}

struct NeedleVisitor<'a> {
    needle: &'a str,
    matched: bool,
}

impl<'a> tracing::field::Visit for NeedleVisitor<'a> {
    fn record_str(&mut self, _field: &tracing::field::Field, value: &str) {
        if value.contains(self.needle) {
            self.matched = true;
        }
    }
    fn record_debug(&mut self, _field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.matched {
            let s = format!("{:?}", value);
            if s.contains(self.needle) {
                self.matched = true;
            }
        }
    }
}

impl<S: Subscriber> Layer<S> for WarnCounter {
    fn on_event(&self, event: &Event<'_>, _ctx: LayerContext<'_, S>) {
        let mut v = NeedleVisitor { needle: WARN_NEEDLE, matched: false };
        event.record(&mut v);
        if v.matched {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn read_self_rss_kb() -> u64 {
    let content = match std::fs::read_to_string("/proc/self/status") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()
                .and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }
    0
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args().map_err(|e| anyhow!("arg parse: {}", e))?;

    if !args.manifest.exists() {
        bail!("manifest not found: {} (try --manifest)", args.manifest.display());
    }

    let warn_count = Arc::new(AtomicU64::new(0));
    let warn_layer = WarnCounter { count: warn_count.clone() };

    let env_filter = tracing_subscriber::EnvFilter::try_new(&args.rust_log)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // fmt layer goes to stderr; TSV output goes to stdout (or file). They
    // don't collide, and we get the usual tracing output for diagnosis if
    // anything goes wrong.
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(warn_layer)
        .init();

    eprintln!("wedge-observe: manifest={} timeout={}s channel_capacity={}",
        args.manifest.display(), args.timeout_sec, CHANNEL_CAPACITY);

    // Parse manifest + load wasm bytes (same path-resolution rules as the
    // CLI's spawn command — relative `package` is resolved against the
    // manifest's directory).
    let manifest_content = tokio::fs::read_to_string(&args.manifest).await
        .with_context(|| format!("read manifest: {}", args.manifest.display()))?;
    let manifest = ManifestConfig::from_toml_str(&manifest_content)
        .with_context(|| "parse manifest")?;

    let wasm_path = if manifest.package.starts_with('/') || manifest.package.contains("://") {
        manifest.package.clone()
    } else {
        let manifest_dir = args.manifest.parent()
            .ok_or_else(|| anyhow!("manifest has no parent directory"))?;
        manifest_dir.join(&manifest.package).to_string_lossy().to_string()
    };
    let wasm_bytes = resolve_reference(&wasm_path).await
        .map_err(|e| anyhow!("load wasm: {}", e))?;

    // Build the runtime. Minimal handler set — only what the wedge-repro
    // supervisor + noisy-child actually import.
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(CHANNEL_CAPACITY);
    let theater_tx_for_metric = theater_tx.clone();

    let mut registry = HandlerRegistry::new();
    registry.register(RuntimeHandler::new(RuntimeHostConfig {}, theater_tx.clone(), None));
    registry.register(SupervisorHandler::new(SupervisorHostConfig {}, None));

    let mut runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx, None, registry).await
        .map_err(|e| anyhow!("create runtime: {}", e))?;

    let runtime_handle = tokio::spawn(async move {
        if let Err(e) = runtime.run().await {
            eprintln!("wedge-observe: theater runtime error: {}", e);
        }
    });

    // SpawnActor for the supervisor manifest. supervisor_tx receives the
    // actor's final result when it exits — we use that as our "alive" signal.
    let (response_tx, response_rx) = oneshot::channel();
    let (supervisor_result_tx, mut supervisor_result_rx) = mpsc::channel(8);

    let init_state = match manifest.initial_state.as_ref() {
        Some(s) => Value::String(s.clone()),
        None => default_init_state(),
    };

    let name = manifest.name.clone();
    let cmd = TheaterCommand::SpawnActor {
        wasm_bytes,
        name: Some(name.clone()),
        manifest: Some(manifest),
        init_state,
        response_tx,
        supervisor_tx: Some(supervisor_result_tx),
        subscription_tx: None,
    };

    theater_tx.send(cmd).await.map_err(|e| anyhow!("send spawn: {}", e))?;

    let actor_id = match response_rx.await {
        Ok(Ok(id)) => id,
        Ok(Err(e)) => bail!("spawn failed: {}", e),
        Err(e) => bail!("spawn response channel: {}", e),
    };
    eprintln!("wedge-observe: spawned {} actor_id={}", name, actor_id);

    // Output sink
    let mut out: Box<dyn Write + Send> = match &args.output {
        Some(p) => Box::new(std::fs::File::create(p)
            .with_context(|| format!("create output: {}", p.display()))?),
        None => Box::new(std::io::stdout()),
    };
    writeln!(out, "elapsed_ms\trss_kb\tch_depth\tch_cap\twarn_total\twarn_delta\talive")?;

    // Sampling loop
    let start = Instant::now();
    let timeout = Duration::from_secs(args.timeout_sec);
    let mut interval = tokio::time::interval(Duration::from_millis(SAMPLE_INTERVAL_MS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut prev_warns: u64 = 0;
    let mut supervisor_alive = true;
    let mut exit_reason = "timeout";

    loop {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            break;
        }
        if !supervisor_alive {
            exit_reason = "supervisor_exit";
            break;
        }

        tokio::select! {
            _ = interval.tick() => {
                let elapsed_ms = start.elapsed().as_millis();
                let rss_kb = read_self_rss_kb();
                let ch_cap = theater_tx_for_metric.capacity();
                let ch_depth = CHANNEL_CAPACITY.saturating_sub(ch_cap);
                let warn_total = warn_count.load(Ordering::Relaxed);
                let warn_delta = warn_total.saturating_sub(prev_warns);
                prev_warns = warn_total;
                writeln!(out, "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    elapsed_ms, rss_kb, ch_depth, ch_cap, warn_total, warn_delta,
                    if supervisor_alive { 1 } else { 0 })?;
            }
            result = supervisor_result_rx.recv() => {
                match result {
                    Some(res) => {
                        eprintln!("wedge-observe: supervisor result: {:?}", res);
                        supervisor_alive = false;
                    }
                    None => {
                        // channel closed without a result — runtime dropped the sender
                        eprintln!("wedge-observe: supervisor result channel closed");
                        supervisor_alive = false;
                        exit_reason = "supervisor_channel_closed";
                    }
                }
            }
        }
    }

    // Final sample line
    let elapsed_ms = start.elapsed().as_millis();
    let warn_total = warn_count.load(Ordering::Relaxed);
    let ch_cap = theater_tx_for_metric.capacity();
    let ch_depth = CHANNEL_CAPACITY.saturating_sub(ch_cap);
    writeln!(out, "# {} elapsed_ms={} warn_total={} ch_depth={} ch_cap={}",
        exit_reason, elapsed_ms, warn_total, ch_depth, ch_cap)?;
    out.flush()?;

    runtime_handle.abort();
    let _ = runtime_handle.await;

    Ok(())
}
