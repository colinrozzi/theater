//! Native OS process handler for Theater actors.
//!
//! Spawns programs and hands the actor their stdio (pipes or a PTY), signals,
//! and exit status. A raw primitive and deliberately NOT a sandbox: the child
//! is an ordinary process with the host user's authority. Isolation comes from
//! what the actor spawns (bwrap, `podman exec`, a VM, ssh), never from here.
//! See `process.pact` for the interface.
//!
//! Design notes:
//! - Ownership: every process is owned by the actor that spawned it (or was
//!   handed it via `transfer`); other actors can't see or touch it. When the
//!   owner stops, its processes are terminated — SIGTERM to the process group,
//!   then SIGKILL after `kill_grace_ms` — and their I/O tasks are joined before
//!   the handler acks shutdown (the #225 resource-release-before-ack pattern).
//! - Process groups: each child leads its own group (`process_group(0)`, or
//!   `setsid` for a PTY), so `signal`/`kill`/teardown reach its descendants.
//! - Output is never dropped (unlike UDP): readers push into a BOUNDED queue
//!   and block when it is full, so a slow actor backpressures the child exactly
//!   like a full pipe. One ordered queue per process carries stdout and stderr
//!   chunks, then an end marker — which is what lets active mode promise
//!   `on-exit` arrives after the last `on-output`.
//! - A grandchild holding the pipe open must not hang output forever: once the
//!   process has exited, a stream that stays silent for [`DRAIN_GRACE`] is
//!   treated as finished.
//! - Replay needs nothing here: every host call's I/O and every callback is
//!   recorded on the chain by the runtime, so a replay never spawns anything.

mod pty;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::os::fd::OwnedFd;
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use theater::actor::handle::ActorHandle;
use theater::config::actor_manifest::HandlerConfig;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::pack_bridge::{
    pact_result_host_fn, parse_pact, InterfaceImpl, TypeHash, Value, ValueType,
};
use theater::shutdown::ShutdownReceiver;

use pty::PtyMaster;

/// Max bytes per output chunk.
const READ_CHUNK: usize = 64 * 1024;
/// Bounded output queue per process (chunks). Full => readers block => the
/// child blocks on write. Never dropped.
const OUTPUT_QUEUE_CAP: usize = 64;
/// After the process exits, a stream silent this long is considered finished
/// (a backgrounded grandchild may hold the write end open indefinitely).
const DRAIN_GRACE: Duration = Duration::from_millis(500);
const DEFAULT_KILL_GRACE_MS: u64 = 2_000;
/// After SIGKILL at teardown, how long to wait for the exit to be reaped.
const REAP_GRACE: Duration = Duration::from_secs(1);
const DEFAULT_MAX_RUN_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_PTY_TERM: &str = "xterm-256color";
/// The EOF character a PTY in canonical mode turns into end-of-input.
const PTY_EOF: u8 = 0x04;

// ============================================================================
// Config (carries the permission/limit surface, per the tcp/udp precedent)
// ============================================================================

/// Configuration + permission limits for the process handler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessHandlerConfig {
    /// Programs that may be spawned, matched exactly against `spawn-spec.program`
    /// (`"*"` matches anything). `None` = any program. A policy, not a wall: an
    /// allowed shell can launch anything.
    #[serde(default)]
    pub allowed_programs: Option<Vec<String>>,
    /// Max processes this actor may own at once (running or not yet closed).
    /// `None` = unlimited.
    #[serde(default)]
    pub max_processes: Option<u32>,
    /// Whether children inherit the host's environment. When false every child
    /// starts from an empty environment plus its `spawn-spec.env`.
    #[serde(default = "default_true")]
    pub inherit_env: bool,
    /// On owner stop: how long after SIGTERM before SIGKILL. Default 2000ms.
    #[serde(default)]
    pub kill_grace_ms: Option<u64>,
    /// Cap on stdout+stderr collected by `run`; beyond it output is discarded
    /// and `truncated` set. Default 16 MiB.
    #[serde(default)]
    pub max_run_output_bytes: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl Default for ProcessHandlerConfig {
    fn default() -> Self {
        Self {
            allowed_programs: None,
            max_processes: None,
            inherit_env: true,
            kill_grace_ms: None,
            max_run_output_bytes: None,
        }
    }
}

impl ProcessHandlerConfig {
    fn check_program(&self, program: &str) -> Result<(), Value> {
        if let Some(ref allowed) = self.allowed_programs {
            if !allowed.iter().any(|a| a == "*" || a == program) {
                return Err(Value::String(format!(
                    "permission denied: program {program:?} not allowed"
                )));
            }
        }
        Ok(())
    }
    fn kill_grace(&self) -> Duration {
        Duration::from_millis(self.kill_grace_ms.unwrap_or(DEFAULT_KILL_GRACE_MS))
    }
    fn max_run_output(&self) -> usize {
        self.max_run_output_bytes
            .unwrap_or(DEFAULT_MAX_RUN_OUTPUT_BYTES) as usize
    }
}

// ============================================================================

/// Drops at scope exit, emitting one `phase=… elapsed_ms=…` debug line per host
/// fn invocation regardless of return path.
struct PhaseLog {
    name: &'static str,
    start: Instant,
}
impl PhaseLog {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}
impl Drop for PhaseLog {
    fn drop(&mut self) {
        debug!(
            phase = self.name,
            elapsed_ms = self.start.elapsed().as_millis() as u64,
            "process phase complete",
        );
    }
}

const PROCESS_PACT: &str = include_str!("../process.pact");

/// Declare the `theater:simple/process` interface from the embedded pact.
fn process_interface() -> InterfaceImpl {
    let pact = parse_pact(PROCESS_PACT).expect("embedded process.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Spawn spec
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    Pipe,
    Null,
}

impl Disposition {
    fn parse(field: &str, s: &str) -> Result<Self, Value> {
        match s {
            "" | "pipe" => Ok(Disposition::Pipe),
            "null" => Ok(Disposition::Null),
            other => Err(Value::String(format!(
                "spawn-spec.{field}: invalid disposition {other:?} (want pipe|null)"
            ))),
        }
    }
    fn stdio(self) -> Stdio {
        match self {
            Disposition::Pipe => Stdio::piped(),
            Disposition::Null => Stdio::null(),
        }
    }
}

#[derive(Debug, Clone)]
struct SpawnSpec {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    env: Vec<(String, String)>,
    clear_env: bool,
    pty: Option<(u16, u16)>,
    stdin: Disposition,
    stdout: Disposition,
    stderr: Disposition,
}

fn strings(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn parse_spawn_spec(input: &Value) -> Result<SpawnSpec, Value> {
    let fields = match input {
        Value::Record { fields, .. } => fields,
        Value::Tuple(t) if t.len() == 1 => return parse_spawn_spec(&t[0]),
        _ => return Err(Value::String("expected spawn-spec record".into())),
    };
    let mut spec = SpawnSpec {
        program: String::new(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        clear_env: false,
        pty: None,
        stdin: Disposition::Pipe,
        stdout: Disposition::Pipe,
        stderr: Disposition::Pipe,
    };
    for (key, val) in fields {
        match (key.as_str(), val) {
            ("program", Value::String(s)) => spec.program = s.clone(),
            ("args", Value::List { items, .. }) => spec.args = strings(items),
            ("cwd", Value::Option { value, .. }) => {
                spec.cwd = match value.as_deref() {
                    None => None,
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(_) => return Err(Value::String("spawn-spec.cwd: expected string".into())),
                }
            }
            ("env", Value::List { items, .. }) => {
                for item in items {
                    if let Value::Tuple(t) = item {
                        if let [Value::String(k), Value::String(v)] = t.as_slice() {
                            spec.env.push((k.clone(), v.clone()));
                        }
                    }
                }
            }
            ("clear-env", Value::Bool(b)) => spec.clear_env = *b,
            ("pty", Value::Option { value, .. }) => {
                spec.pty = match value.as_deref() {
                    None => None,
                    Some(Value::Tuple(t)) => match t.as_slice() {
                        [Value::U16(cols), Value::U16(rows)] => Some((*cols, *rows)),
                        _ => {
                            return Err(Value::String(
                                "spawn-spec.pty: expected (cols: u16, rows: u16)".into(),
                            ))
                        }
                    },
                    Some(_) => {
                        return Err(Value::String(
                            "spawn-spec.pty: expected (cols: u16, rows: u16)".into(),
                        ))
                    }
                }
            }
            ("stdin", Value::String(s)) => spec.stdin = Disposition::parse("stdin", s)?,
            ("stdout", Value::String(s)) => spec.stdout = Disposition::parse("stdout", s)?,
            ("stderr", Value::String(s)) => spec.stderr = Disposition::parse("stderr", s)?,
            _ => {}
        }
    }
    if spec.program.is_empty() {
        return Err(Value::String("spawn-spec.program is required".into()));
    }
    Ok(spec)
}

// ── parsing helpers ─────────────────────────────────────────────────────────

fn parse_string(input: &Value) -> Result<String, Value> {
    match input {
        Value::String(s) => Ok(s.clone()),
        Value::Tuple(f) if f.len() == 1 => match &f[0] {
            Value::String(s) => Ok(s.clone()),
            _ => Err(Value::String("expected string".into())),
        },
        _ => Err(Value::String("expected string".into())),
    }
}
fn tuple(input: &Value, n: usize) -> Result<&[Value], Value> {
    match input {
        Value::Tuple(f) if f.len() == n => Ok(f),
        _ => Err(Value::String(format!("expected tuple of {n} args"))),
    }
}
fn str_at(fields: &[Value], i: usize) -> Result<String, Value> {
    match fields.get(i) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(Value::String(format!("expected string at arg {i}"))),
    }
}
fn u16_at(fields: &[Value], i: usize) -> Result<u16, Value> {
    match fields.get(i) {
        Some(Value::U16(n)) => Ok(*n),
        _ => Err(Value::String(format!("expected u16 at arg {i}"))),
    }
}
fn u32_at(fields: &[Value], i: usize) -> Result<u32, Value> {
    match fields.get(i) {
        Some(Value::U32(n)) => Ok(*n),
        _ => Err(Value::String(format!("expected u32 at arg {i}"))),
    }
}
fn s32_at(fields: &[Value], i: usize) -> Result<i32, Value> {
    match fields.get(i) {
        Some(Value::S32(n)) => Ok(*n),
        _ => Err(Value::String(format!("expected s32 at arg {i}"))),
    }
}
fn opt_u64_at(fields: &[Value], i: usize) -> Result<Option<u64>, Value> {
    match fields.get(i) {
        Some(Value::Option { value: None, .. }) => Ok(None),
        Some(Value::Option { value: Some(v), .. }) => match v.as_ref() {
            Value::U64(n) => Ok(Some(*n)),
            _ => Err(Value::String(format!("expected option<u64> at arg {i}"))),
        },
        _ => Err(Value::String(format!("expected option<u64> at arg {i}"))),
    }
}
fn bytes_at(fields: &[Value], i: usize) -> Result<Vec<u8>, Value> {
    match fields.get(i) {
        Some(Value::List { items, .. }) => Ok(items
            .iter()
            .filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None })
            .collect()),
        _ => Err(Value::String(format!("expected list<u8> at arg {i}"))),
    }
}

fn id_to_string(id: u64) -> String {
    id.to_string()
}
fn string_to_id(s: &str) -> Result<u64, Value> {
    s.parse::<u64>()
        .map_err(|_| Value::String(format!("invalid process-id: {s}")))
}

// ── value ctors (pact_result_host_fn wraps the success side as result::ok) ──

fn ok_unit() -> Value {
    Value::Tuple(vec![])
}
fn bytes_value(data: Vec<u8>) -> Value {
    Value::List {
        elem_type: ValueType::U8,
        items: data.into_iter().map(Value::U8).collect(),
    }
}
fn opt_s32(v: Option<i32>) -> Value {
    Value::Option {
        inner_type: ValueType::S32,
        value: v.map(|n| Box::new(Value::S32(n))),
    }
}
fn status_value(exit: Option<ExitInfo>) -> Value {
    Value::Record {
        type_name: "process-status".to_string(),
        fields: vec![
            ("running".to_string(), Value::Bool(exit.is_none())),
            ("exit-code".to_string(), opt_s32(exit.and_then(|e| e.code))),
            ("signal".to_string(), opt_s32(exit.and_then(|e| e.signal))),
        ],
    }
}
fn chunk_value(stream: &str, data: Vec<u8>) -> Value {
    Value::Record {
        type_name: "output-chunk".to_string(),
        fields: vec![
            ("stream".to_string(), Value::String(stream.to_string())),
            ("data".to_string(), bytes_value(data)),
        ],
    }
}

// ============================================================================
// Process plumbing
// ============================================================================

/// How a process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExitInfo {
    code: Option<i32>,
    signal: Option<i32>,
}

fn exit_info(status: ExitStatus) -> ExitInfo {
    use std::os::unix::process::ExitStatusExt;
    ExitInfo {
        code: status.code(),
        signal: status.signal(),
    }
}

/// One output chunk: `("stdout" | "stderr", bytes)`.
type Chunk = (&'static str, Vec<u8>);

/// A process's ordered output. Only the stream readers hold senders, so the
/// channel closing IS the end-of-output marker: it closes exactly when every
/// stream has finished.
struct OutputQueue {
    rx: mpsc::Receiver<Chunk>,
    /// Remainder of a chunk split by a short passive read.
    pending: Option<Chunk>,
    /// The channel closed: no more output will ever arrive.
    done: bool,
    /// `on-exit` was pushed, so a later set-active doesn't repeat it.
    exit_delivered: bool,
}

impl OutputQueue {
    fn new(rx: mpsc::Receiver<Chunk>) -> Self {
        Self {
            rx,
            pending: None,
            done: false,
            exit_delivered: false,
        }
    }

    /// The next chunk, or `None` once all output is consumed. Cancel-safe.
    async fn next(&mut self) -> Option<Chunk> {
        if let Some(p) = self.pending.take() {
            return Some(p);
        }
        if self.done {
            return None;
        }
        match self.rx.recv().await {
            Some(chunk) => Some(chunk),
            None => {
                self.done = true;
                None
            }
        }
    }
}

/// Resolves [`DRAIN_GRACE`] after the process has exited.
async fn exited_plus_grace(mut status: watch::Receiver<Option<ExitInfo>>) {
    // Err = the waiter is gone (teardown); treat as exited.
    let _ = status.wait_for(|s| s.is_some()).await;
    tokio::time::sleep(DRAIN_GRACE).await;
}

/// Pump one pipe into the queue until EOF, or until it has stayed silent for
/// [`DRAIN_GRACE`] after the process exited.
async fn read_stream<R: AsyncRead + Unpin>(
    mut reader: R,
    stream: &'static str,
    tx: mpsc::Sender<Chunk>,
    status: watch::Receiver<Option<ExitInfo>>,
) {
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = tokio::select! {
            r = reader.read(&mut buf) => match r {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            },
            _ = exited_plus_grace(status.clone()) => break,
        };
        if tx.send((stream, buf[..n].to_vec())).await.is_err() {
            break;
        }
    }
}

/// [`read_stream`] for a PTY master (one combined stream, reported as stdout).
async fn read_pty(
    master: Arc<PtyMaster>,
    tx: mpsc::Sender<Chunk>,
    status: watch::Receiver<Option<ExitInfo>>,
) {
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = tokio::select! {
            r = master.read(&mut buf) => match r {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            },
            _ = exited_plus_grace(status.clone()) => break,
        };
        if tx.send(("stdout", buf[..n].to_vec())).await.is_err() {
            break;
        }
    }
}

/// A freshly started process and the tasks driving it.
struct Spawned {
    os_pid: u32,
    pty: Option<Arc<PtyMaster>>,
    stdin: Option<ChildStdin>,
    output: OutputQueue,
    status: watch::Receiver<Option<ExitInfo>>,
    /// The stream readers and the exit waiter (which owns the
    /// `Child`; aborting it kills the child via `kill_on_drop`).
    tasks: Vec<JoinHandle<()>>,
}

fn spawn_process(spec: &SpawnSpec, cfg: &ProcessHandlerConfig) -> Result<Spawned, String> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    if spec.clear_env || !cfg.inherit_env {
        cmd.env_clear();
    }
    if spec.pty.is_some() && !spec.env.iter().any(|(k, _)| k == "TERM") {
        cmd.env("TERM", DEFAULT_PTY_TERM);
    }
    cmd.envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    cmd.kill_on_drop(true);

    let (tx, rx) = mpsc::channel(OUTPUT_QUEUE_CAP);
    let (status_tx, status_rx) = watch::channel(None);
    let mut readers = Vec::new();

    let spawn_err = |e: std::io::Error| format!("spawn {:?} failed: {e}", spec.program);
    let (child, pty, stdin) = if let Some((cols, rows)) = spec.pty {
        let (master, slave) =
            pty::open_pty(cols, rows).map_err(|e| format!("openpty failed: {e}"))?;
        let dup = |fd: &OwnedFd| {
            fd.try_clone()
                .map(Stdio::from)
                .map_err(|e| format!("dup pty slave failed: {e}"))
        };
        cmd.stdin(dup(&slave)?)
            .stdout(dup(&slave)?)
            .stderr(Stdio::from(slave));
        // SAFETY: make_controlling_tty only calls setsid/ioctl, which are
        // async-signal-safe.
        unsafe {
            cmd.pre_exec(pty::make_controlling_tty);
        }
        let child = cmd.spawn().map_err(spawn_err)?;
        let master =
            Arc::new(PtyMaster::new(master).map_err(|e| format!("pty master setup failed: {e}"))?);
        readers.push(tokio::spawn(read_pty(
            master.clone(),
            tx.clone(),
            status_rx.clone(),
        )));
        (child, Some(master), None)
    } else {
        cmd.process_group(0)
            .stdin(spec.stdin.stdio())
            .stdout(spec.stdout.stdio())
            .stderr(spec.stderr.stdio());
        let mut child = cmd.spawn().map_err(spawn_err)?;
        if let Some(out) = child.stdout.take() {
            readers.push(tokio::spawn(read_stream(
                out,
                "stdout",
                tx.clone(),
                status_rx.clone(),
            )));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(tokio::spawn(read_stream(
                err,
                "stderr",
                tx.clone(),
                status_rx.clone(),
            )));
        }
        let stdin = child.stdin.take();
        (child, None, stdin)
    };
    // Release the parent's copies of the PTY slave now: the master only sees
    // EOF once every slave fd is closed.
    drop(cmd);

    let os_pid = child
        .id()
        .ok_or_else(|| "spawned process has no pid".to_string())?;
    let program = spec.program.clone();
    let waiter = tokio::spawn(async move {
        let mut child = child;
        let info = match child.wait().await {
            Ok(st) => exit_info(st),
            Err(e) => {
                warn!("process {} (pid {}) wait failed: {}", program, os_pid, e);
                ExitInfo {
                    code: None,
                    signal: None,
                }
            }
        };
        let _ = status_tx.send(Some(info));
    });

    // From here only the readers hold senders (see OutputQueue).
    drop(tx);
    let mut tasks = readers;
    tasks.push(waiter);

    Ok(Spawned {
        os_pid,
        pty,
        stdin,
        output: OutputQueue::new(rx),
        status: status_rx,
        tasks,
    })
}

/// Signal a process group. A group that no longer exists is not an error.
fn signal_group(pgid: u32, sig: i32) -> Result<(), String> {
    let r = unsafe { libc::kill(-(pgid as libc::pid_t), sig) };
    if r == -1 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(format!("signal {sig} to process group {pgid} failed: {e}"));
    }
    Ok(())
}

/// Resolves after `ms`, or never.
async fn sleep_opt(ms: Option<u64>) {
    match ms {
        Some(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
        None => std::future::pending::<()>().await,
    }
}

async fn wait_exit(status: &mut watch::Receiver<Option<ExitInfo>>) -> ExitInfo {
    match status.wait_for(|s| s.is_some()).await {
        Ok(s) => s.expect("wait_for guarantees Some"),
        Err(_) => ExitInfo {
            code: None,
            signal: None,
        },
    }
}

// ============================================================================
// run(): spawn + feed stdin + collect + wait
// ============================================================================

struct RunOutput {
    exit: ExitInfo,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
}

impl RunOutput {
    fn into_value(self) -> Value {
        Value::Record {
            type_name: "run-output".to_string(),
            fields: vec![
                ("status".to_string(), status_value(Some(self.exit))),
                ("stdout".to_string(), bytes_value(self.stdout)),
                ("stderr".to_string(), bytes_value(self.stderr)),
                ("truncated".to_string(), Value::Bool(self.truncated)),
            ],
        }
    }
}

/// Kills the process group and aborts its tasks when dropped — so a timed-out
/// or cancelled `run` (actor shutdown drops the host call) leaves nothing
/// behind.
struct RunGuard {
    pgid: u32,
    armed: bool,
    tasks: Vec<JoinHandle<()>>,
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = signal_group(self.pgid, libc::SIGKILL);
        }
        for t in &self.tasks {
            t.abort();
        }
    }
}

async fn run_process(
    spec: &SpawnSpec,
    cfg: &ProcessHandlerConfig,
    input: Vec<u8>,
    timeout_ms: Option<u64>,
) -> Result<RunOutput, String> {
    let Spawned {
        os_pid,
        pty,
        stdin,
        mut output,
        mut status,
        tasks,
    } = spawn_process(spec, cfg)?;
    let mut guard = RunGuard {
        pgid: os_pid,
        armed: true,
        tasks,
    };
    let cap = cfg.max_run_output();

    let work = async {
        // Feed stdin concurrently with draining output: a child that writes
        // before it has read all its input would otherwise deadlock us.
        let feed = async {
            if let Some(master) = &pty {
                if !input.is_empty() {
                    let _ = master.write_all(&input).await;
                }
                let _ = master.write_all(&[PTY_EOF]).await;
            } else if let Some(mut w) = stdin {
                if !input.is_empty() {
                    // EPIPE (child doesn't read stdin) is fine.
                    let _ = w.write_all(&input).await;
                }
                drop(w); // EOF
            }
        };
        let collect = async {
            let (mut out, mut err, mut truncated) = (Vec::new(), Vec::new(), false);
            while let Some((stream, data)) = output.next().await {
                if out.len() + err.len() + data.len() > cap {
                    truncated = true; // keep draining so the child never blocks
                    continue;
                }
                if stream == "stderr" {
                    err.extend_from_slice(&data);
                } else {
                    out.extend_from_slice(&data);
                }
            }
            (out, err, truncated)
        };
        let ((), (stdout, stderr, truncated)) = tokio::join!(feed, collect);
        let exit = wait_exit(&mut status).await;
        RunOutput {
            exit,
            stdout,
            stderr,
            truncated,
        }
    };

    tokio::select! {
        out = work => {
            guard.armed = false; // finished: leave any detached descendants alone
            Ok(out)
        }
        _ = sleep_opt(timeout_ms) => Err(format!(
            "timeout: {:?} did not finish within {}ms (killed)",
            spec.program,
            timeout_ms.unwrap_or(0)
        )),
    }
}

// ============================================================================
// Process table
// ============================================================================

/// Output mode (mirrors the tcp/udp handlers' Erlang-style model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataMode {
    Passive,
    Active,
    Once,
}

/// A tracked process with ownership + mode.
struct ProcessEntry {
    owner: TheaterId,
    os_pid: u32,
    pty: Option<Arc<PtyMaster>>,
    /// Piped stdin; `None` inside once closed.
    stdin: Option<Arc<Mutex<Option<ChildStdin>>>>,
    output: Arc<Mutex<OutputQueue>>,
    status: watch::Receiver<Option<ExitInfo>>,
    mode: DataMode,
    /// Active/once delivery task.
    pump: Option<(CancellationToken, JoinHandle<()>)>,
    tasks: Vec<JoinHandle<()>>,
}

impl ProcessEntry {
    fn exit(&self) -> Option<ExitInfo> {
        *self.status.borrow()
    }

    /// Stop push delivery. Cancel rather than abort: a delivery already in
    /// flight completes (the chunk was dequeued, so aborting would lose it).
    fn stop_pump(&mut self) {
        if let Some((cancel, _)) = self.pump.take() {
            cancel.cancel();
        }
    }

    /// Abort every task (teardown) and hand back the handles to join.
    fn abort_all(&mut self) -> Vec<JoinHandle<()>> {
        let mut handles: Vec<JoinHandle<()>> = self.tasks.drain(..).collect();
        if let Some((cancel, pump)) = self.pump.take() {
            cancel.cancel();
            handles.push(pump);
        }
        for h in &handles {
            h.abort();
        }
        handles
    }
}

/// Shared process state across all handler instances in a runtime (shared so
/// `transfer` can move a process between actors).
struct SharedProcessState {
    processes: Mutex<HashMap<u64, ProcessEntry>>,
    next_id: AtomicU64,
}

impl SharedProcessState {
    fn new() -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }
    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

/// Look up a process owned by `actor`.
fn owned<'a>(
    procs: &'a mut HashMap<u64, ProcessEntry>,
    pid_str: &str,
    actor: TheaterId,
) -> Result<(u64, &'a mut ProcessEntry), Value> {
    let id = string_to_id(pid_str)?;
    let entry = procs
        .get_mut(&id)
        .ok_or_else(|| Value::String(format!("process not found: {pid_str}")))?;
    if entry.owner != actor {
        return Err(Value::String(format!(
            "process {pid_str} not owned by this actor"
        )));
    }
    Ok((id, entry))
}

/// Terminate processes whose owner is going away: SIGTERM each still-running
/// group, wait up to `grace` for them to exit, then SIGKILL every group (also
/// reaching descendants that outlived their leader) and abort + join all I/O
/// tasks so fds are released before returning.
async fn terminate(mut entries: Vec<ProcessEntry>, grace: Duration) {
    for e in &entries {
        if e.exit().is_none() {
            let _ = signal_group(e.os_pid, libc::SIGTERM);
        }
    }
    let deadline = tokio::time::Instant::now() + grace;
    for e in &entries {
        let mut status = e.status.clone();
        let _ = tokio::time::timeout_at(deadline, status.wait_for(|s| s.is_some())).await;
    }
    for e in &entries {
        let _ = signal_group(e.os_pid, libc::SIGKILL);
    }
    // Let the waiters reap what we just killed, so nothing lingers as a zombie.
    let deadline = tokio::time::Instant::now() + REAP_GRACE;
    for e in &entries {
        let mut status = e.status.clone();
        let _ = tokio::time::timeout_at(deadline, status.wait_for(|s| s.is_some())).await;
    }
    let mut handles = Vec::new();
    for e in &mut entries {
        handles.extend(e.abort_all());
    }
    for h in handles {
        let _ = h.await;
    }
}

// ============================================================================
// Active-mode delivery
// ============================================================================

async fn deliver(actor: &ActorHandle, func: &str, params: Value, pid: &str) {
    if let Err(e) = actor
        .call_function(format!("theater:simple/process-client.{func}"), params)
        .await
    {
        warn!("process={} {} delivery failed: {}", pid, func, e);
    }
}

/// Push output to the actor as `on-output` calls, then `on-exit` once the
/// output has ended. `once` stops after a single callback.
async fn output_pump(
    pid: String,
    output: Arc<Mutex<OutputQueue>>,
    mut status: watch::Receiver<Option<ExitInfo>>,
    actor: ActorHandle,
    once: bool,
    cancel: CancellationToken,
) {
    loop {
        // Hold the queue lock only while dequeuing, never across delivery: the
        // actor may call read/set-active from inside a callback.
        let next = {
            let mut q = output.lock().await;
            tokio::select! {
                _ = cancel.cancelled() => return,
                n = q.next() => n,
            }
        };
        match next {
            Some((stream, data)) => {
                let params = Value::Tuple(vec![
                    Value::String(pid.clone()),
                    Value::String(stream.to_string()),
                    bytes_value(data),
                ]);
                deliver(&actor, "on-output", params, &pid).await;
            }
            None => {
                let exit = tokio::select! {
                    _ = cancel.cancelled() => return,
                    e = wait_exit(&mut status) => e,
                };
                {
                    let mut q = output.lock().await;
                    if q.exit_delivered {
                        return;
                    }
                    q.exit_delivered = true;
                }
                let params =
                    Value::Tuple(vec![Value::String(pid.clone()), status_value(Some(exit))]);
                deliver(&actor, "on-exit", params, &pid).await;
                return;
            }
        }
        if once {
            return;
        }
    }
}

fn start_pump(
    st: Arc<SharedProcessState>,
    id: u64,
    entry: &mut ProcessEntry,
    actor: ActorHandle,
    once: bool,
) {
    let cancel = CancellationToken::new();
    let output = entry.output.clone();
    let status = entry.status.clone();
    let pump_cancel = cancel.clone();
    let handle = tokio::spawn(async move {
        output_pump(
            id_to_string(id),
            output,
            status,
            actor,
            once,
            pump_cancel.clone(),
        )
        .await;
        // once-mode self-reverts to passive when its single delivery is done.
        if once && !pump_cancel.is_cancelled() {
            if let Some(e) = st.processes.lock().await.get_mut(&id) {
                if e.mode == DataMode::Once {
                    e.mode = DataMode::Passive;
                    e.pump = None;
                }
            }
        }
    });
    entry.pump = Some((cancel, handle));
}

// ============================================================================
// Handler
// ============================================================================

/// Handler providing native OS processes (pipes + PTY) to WebAssembly actors.
#[derive(Clone)]
pub struct ProcessHandler {
    config: ProcessHandlerConfig,
    shared_state: Arc<SharedProcessState>,
    actor_id: Arc<std::sync::Mutex<Option<TheaterId>>>,
    actor_handle: Arc<std::sync::Mutex<Option<ActorHandle>>>,
    cancellation_token: CancellationToken,
}

impl ProcessHandler {
    pub fn new(config: ProcessHandlerConfig) -> Self {
        Self {
            config,
            shared_state: Arc::new(SharedProcessState::new()),
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![process_interface()]
    }
}

impl Handler for ProcessHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let cfg = match config {
            Some(c) => c.parse::<ProcessHandlerConfig>().unwrap_or_else(|e| {
                warn!("invalid process handler config: {}; using default", e);
                self.config.clone()
            }),
            None => self.config.clone(),
        };
        Box::new(ProcessHandler {
            config: cfg,
            shared_state: self.shared_state.clone(), // shared Arc across instances
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            cancellation_token: CancellationToken::new(),
        })
    }

    fn name(&self) -> &str {
        "process"
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
        Some(vec!["theater:simple/process-client".to_string()])
    }
    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }
    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![process_interface()]
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Process handler setup");
        {
            let mut g = self.actor_handle.lock().unwrap();
            *g = Some(actor_handle);
        }
        let cancel_token = self.cancellation_token.clone();
        let shared_state = self.shared_state.clone();
        let actor_id_for_cleanup = self.actor_id.clone();
        let grace = self.config.kill_grace();

        Box::pin(async move {
            // BIND (do not drop) the shutdown signal: the ack fires on drop, and
            // the runtime awaits it. Hold it until this actor's processes are
            // gone and their fds released (the #225 discipline).
            let _shutdown_signal = shutdown_receiver.wait_for_shutdown().await;
            info!("Process handler received shutdown, cleaning up");
            cancel_token.cancel();

            let actor_id_val = *actor_id_for_cleanup.lock().unwrap();
            let owned: Vec<ProcessEntry> = {
                let mut procs = shared_state.processes.lock().await;
                let ids: Vec<u64> = procs
                    .iter()
                    .filter(|(_, e)| Some(e.owner) == actor_id_val)
                    .map(|(id, _)| *id)
                    .collect();
                ids.into_iter().filter_map(|id| procs.remove(&id)).collect()
            };
            let n = owned.len();
            terminate(owned, grace).await;
            if n > 0 {
                info!(
                    "Process handler terminated {} process(es) for actor {:?}",
                    n, actor_id_val
                );
            }
            Ok(())
        })
    }

    fn register_host_functions(
        &mut self,
        imports: &mut theater::pack_bridge::HostImports,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up process host functions");
        if ctx.is_satisfied("theater:simple/process") {
            return Ok(());
        }
        let actor_id = ctx
            .actor_id
            .ok_or_else(|| anyhow::anyhow!("actor_id not set in HandlerContext"))?;
        {
            let mut g = self.actor_id.lock().unwrap();
            *g = Some(actor_id);
        }
        // Only `transfer` needs the theater command channel (to reach the
        // target actor); it errors at call time if absent.
        let theater_tx = ctx.theater_tx.clone();

        let st = self.shared_state.clone();
        let cfg = self.config.clone();
        let actor_handle_arc = self.actor_handle.clone();
        let cancel = self.cancellation_token.clone();

        // ---- spawn ----
        {
            let st = st.clone();
            let cfg = cfg.clone();
            imports.define(
                "theater:simple/process",
                "spawn",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cfg = cfg.clone();
                    async move {
                        let _ph = PhaseLog::new("process.spawn");
                        let spec = parse_spawn_spec(&input)?;
                        cfg.check_program(&spec.program)?;
                        if let Some(max) = cfg.max_processes {
                            let n = st
                                .processes
                                .lock()
                                .await
                                .values()
                                .filter(|e| e.owner == actor_id)
                                .count();
                            if n >= max as usize {
                                return Err(Value::String(format!(
                                    "permission denied: max processes ({max}) reached"
                                )));
                            }
                        }
                        let sp = spawn_process(&spec, &cfg).map_err(Value::String)?;
                        let id = st.next_id();
                        info!(
                            "process spawned {:?} as process={} (pid {}, pty={})",
                            spec.program,
                            id,
                            sp.os_pid,
                            sp.pty.is_some()
                        );
                        st.processes.lock().await.insert(
                            id,
                            ProcessEntry {
                                owner: actor_id,
                                os_pid: sp.os_pid,
                                pty: sp.pty,
                                stdin: sp.stdin.map(|s| Arc::new(Mutex::new(Some(s)))),
                                output: Arc::new(Mutex::new(sp.output)),
                                status: sp.status,
                                mode: DataMode::Passive,
                                pump: None,
                                tasks: sp.tasks,
                            },
                        );
                        Ok::<Value, Value>(Value::String(id_to_string(id)))
                    }
                }),
            );
        }

        // ---- run ----
        {
            let cfg = cfg.clone();
            let cancel = cancel.clone();
            imports.define(
                "theater:simple/process",
                "run",
                pact_result_host_fn(move |input: Value| {
                    let cfg = cfg.clone();
                    let cancel = cancel.clone();
                    async move {
                        let _ph = PhaseLog::new("process.run");
                        let f = tuple(&input, 3)?;
                        let spec = parse_spawn_spec(&f[0])?;
                        let stdin = bytes_at(f, 1)?;
                        let timeout = opt_u64_at(f, 2)?;
                        cfg.check_program(&spec.program)?;
                        let out = tokio::select! {
                            r = run_process(&spec, &cfg, stdin, timeout) => r.map_err(Value::String)?,
                            _ = cancel.cancelled() => return Err(Value::String(
                                "process handler shutting down".into(),
                            )),
                        };
                        Ok::<Value, Value>(out.into_value())
                    }
                }),
            );
        }

        // ---- write-stdin / close-stdin ----
        {
            let st = st.clone();
            let cancel = cancel.clone();
            imports.define(
                "theater:simple/process",
                "write-stdin",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cancel = cancel.clone();
                    async move {
                        let _ph = PhaseLog::new("process.write-stdin");
                        let f = tuple(&input, 2)?;
                        let pid_str = str_at(f, 0)?;
                        let data = bytes_at(f, 1)?;
                        let (pty, stdin) = {
                            let mut procs = st.processes.lock().await;
                            let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                            (e.pty.clone(), e.stdin.clone())
                        };
                        let write = async {
                            if let Some(master) = pty {
                                return master
                                    .write_all(&data)
                                    .await
                                    .map_err(|e| format!("pty write failed: {e}"));
                            }
                            let stdin = stdin.ok_or("stdin is not piped")?;
                            let mut g = stdin.lock().await;
                            let w = g.as_mut().ok_or("stdin is closed")?;
                            w.write_all(&data)
                                .await
                                .map_err(|e| format!("stdin write failed: {e}"))?;
                            w.flush()
                                .await
                                .map_err(|e| format!("stdin flush failed: {e}"))
                        };
                        tokio::select! {
                            r = write => r.map_err(|e: String| Value::String(e))?,
                            _ = cancel.cancelled() => return Err(Value::String(
                                "process handler shutting down".into(),
                            )),
                        }
                        Ok::<Value, Value>(Value::U64(data.len() as u64))
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "close-stdin",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.close-stdin");
                        let pid_str = parse_string(&input)?;
                        let (pty, stdin) = {
                            let mut procs = st.processes.lock().await;
                            let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                            (e.pty.clone(), e.stdin.clone())
                        };
                        if let Some(master) = pty {
                            master
                                .write_all(&[PTY_EOF])
                                .await
                                .map_err(|e| Value::String(format!("pty write failed: {e}")))?;
                        } else if let Some(stdin) = stdin {
                            stdin.lock().await.take(); // drop => EOF
                        } else {
                            return Err(Value::String("stdin is not piped".into()));
                        }
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- read (passive) ----
        {
            let st = st.clone();
            let cancel = cancel.clone();
            imports.define(
                "theater:simple/process",
                "read",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cancel = cancel.clone();
                    async move {
                        let _ph = PhaseLog::new("process.read");
                        let f = tuple(&input, 3)?;
                        let pid_str = str_at(f, 0)?;
                        let max_bytes = u32_at(f, 1)? as usize;
                        let timeout = opt_u64_at(f, 2)?;
                        if max_bytes == 0 {
                            return Err(Value::String("max-bytes must be > 0".into()));
                        }
                        let output = {
                            let mut procs = st.processes.lock().await;
                            let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                            if e.mode != DataMode::Passive {
                                return Err(Value::String(format!(
                                    "process {pid_str} is in active/once mode - output is pushed to on-output"
                                )));
                            }
                            e.output.clone()
                        };
                        let mut q = output.lock().await;
                        let next = tokio::select! {
                            n = q.next() => n,
                            _ = cancel.cancelled() => return Err(Value::String(
                                "process handler shutting down".into(),
                            )),
                            _ = sleep_opt(timeout) => return Err(Value::String(format!(
                                "timeout: no output within {}ms",
                                timeout.unwrap_or(0)
                            ))),
                        };
                        let chunk = next.map(|(stream, mut data)| {
                            if data.len() > max_bytes {
                                let rest = data.split_off(max_bytes);
                                q.pending = Some((stream, rest));
                            }
                            Box::new(chunk_value(stream, data))
                        });
                        Ok::<Value, Value>(Value::Option {
                            inner_type: ValueType::Record("output-chunk".to_string()),
                            value: chunk,
                        })
                    }
                }),
            );
        }

        // ---- set-active ----
        {
            let st = st.clone();
            let actor_handle_arc = actor_handle_arc.clone();
            imports.define(
                "theater:simple/process",
                "set-active",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let actor_handle_arc = actor_handle_arc.clone();
                    async move {
                        let _ph = PhaseLog::new("process.set-active");
                        let f = tuple(&input, 2)?;
                        let pid_str = str_at(f, 0)?;
                        let mode_s = str_at(f, 1)?;
                        let new_mode = match mode_s.as_str() {
                            "passive" => DataMode::Passive,
                            "active" => DataMode::Active,
                            "once" => DataMode::Once,
                            _ => {
                                return Err(Value::String(format!(
                                    "invalid mode {mode_s:?} (want passive|active|once)"
                                )))
                            }
                        };
                        let actor_handle = actor_handle_arc.lock().unwrap().clone();
                        let mut procs = st.processes.lock().await;
                        let (id, e) = owned(&mut procs, &pid_str, actor_id)?;
                        if e.mode == new_mode && (new_mode == DataMode::Passive || e.pump.is_some())
                        {
                            return Ok::<Value, Value>(ok_unit());
                        }
                        e.stop_pump();
                        e.mode = new_mode;
                        if new_mode != DataMode::Passive {
                            let Some(actor_handle) = actor_handle else {
                                e.mode = DataMode::Passive;
                                return Err(Value::String("actor handle not available".into()));
                            };
                            start_pump(st.clone(), id, e, actor_handle, new_mode == DataMode::Once);
                        }
                        info!("process={} set to {} mode", pid_str, mode_s);
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- resize ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "resize",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.resize");
                        let f = tuple(&input, 3)?;
                        let pid_str = str_at(f, 0)?;
                        let cols = u16_at(f, 1)?;
                        let rows = u16_at(f, 2)?;
                        let mut procs = st.processes.lock().await;
                        let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                        let master = e.pty.as_ref().ok_or_else(|| {
                            Value::String(format!("process {pid_str} has no pty"))
                        })?;
                        master
                            .resize(cols, rows)
                            .map_err(|e| Value::String(format!("resize failed: {e}")))?;
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- signal / kill ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "signal",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.signal");
                        let f = tuple(&input, 2)?;
                        let pid_str = str_at(f, 0)?;
                        let sig = s32_at(f, 1)?;
                        let mut procs = st.processes.lock().await;
                        let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                        if e.exit().is_none() {
                            signal_group(e.os_pid, sig).map_err(Value::String)?;
                        }
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "kill",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.kill");
                        let pid_str = parse_string(&input)?;
                        let mut procs = st.processes.lock().await;
                        let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                        signal_group(e.os_pid, libc::SIGKILL).map_err(Value::String)?;
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- status / wait / os-pid / list ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "status",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.status");
                        let pid_str = parse_string(&input)?;
                        let mut procs = st.processes.lock().await;
                        let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                        Ok::<Value, Value>(status_value(e.exit()))
                    }
                }),
            );
        }
        {
            let st = st.clone();
            let cancel = cancel.clone();
            imports.define(
                "theater:simple/process",
                "wait",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cancel = cancel.clone();
                    async move {
                        let _ph = PhaseLog::new("process.wait");
                        let f = tuple(&input, 2)?;
                        let pid_str = str_at(f, 0)?;
                        let timeout = opt_u64_at(f, 1)?;
                        let mut status = {
                            let mut procs = st.processes.lock().await;
                            let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                            e.status.clone()
                        };
                        let exit = tokio::select! {
                            e = wait_exit(&mut status) => e,
                            _ = cancel.cancelled() => return Err(Value::String(
                                "process handler shutting down".into(),
                            )),
                            _ = sleep_opt(timeout) => return Err(Value::String(format!(
                                "timeout: process {pid_str} still running after {}ms",
                                timeout.unwrap_or(0)
                            ))),
                        };
                        Ok::<Value, Value>(status_value(Some(exit)))
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "os-pid",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.os-pid");
                        let pid_str = parse_string(&input)?;
                        let mut procs = st.processes.lock().await;
                        let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                        Ok::<Value, Value>(Value::U32(e.os_pid))
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "list",
                pact_result_host_fn(move |_input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.list");
                        let mut ids: Vec<u64> = st
                            .processes
                            .lock()
                            .await
                            .iter()
                            .filter(|(_, e)| e.owner == actor_id)
                            .map(|(id, _)| *id)
                            .collect();
                        ids.sort_unstable();
                        Ok::<Value, Value>(Value::List {
                            elem_type: ValueType::String,
                            items: ids
                                .into_iter()
                                .map(|id| Value::String(id_to_string(id)))
                                .collect(),
                        })
                    }
                }),
            );
        }

        // ---- close ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "close",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("process.close");
                        let pid_str = parse_string(&input)?;
                        let mut entry = {
                            let mut procs = st.processes.lock().await;
                            let (id, _) = owned(&mut procs, &pid_str, actor_id)?;
                            procs.remove(&id).expect("just looked up")
                        };
                        let _ = signal_group(entry.os_pid, libc::SIGKILL);
                        // Abort without joining: a pump may be queued behind
                        // this very call on the actor.
                        entry.abort_all();
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- transfer ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/process",
                "transfer",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        let _ph = PhaseLog::new("process.transfer");
                        let f = tuple(&input, 2)?;
                        let pid_str = str_at(f, 0)?;
                        let target_str = str_at(f, 1)?;
                        let target: TheaterId = target_str
                            .parse()
                            .map_err(|e| Value::String(format!("invalid actor id: {e}")))?;
                        let theater_tx = theater_tx.ok_or_else(|| {
                            Value::String("transfer unavailable: no theater channel".into())
                        })?;
                        // Resolve the target before moving anything, so a bad
                        // target leaves the process with its owner.
                        let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorHandle {
                                actor_id: target,
                                response_tx: handle_tx,
                            })
                            .map_err(|e| {
                                Value::String(format!("failed to get target handle: {e}"))
                            })?;
                        let target_handle = match handle_rx.await {
                            Ok(Some(h)) => h,
                            Ok(None) => {
                                return Err(Value::String(format!(
                                    "target actor not found: {target_str}"
                                )))
                            }
                            Err(e) => {
                                return Err(Value::String(format!(
                                    "failed to receive target handle: {e}"
                                )))
                            }
                        };
                        {
                            let mut procs = st.processes.lock().await;
                            let (_, e) = owned(&mut procs, &pid_str, actor_id)?;
                            e.stop_pump();
                            e.mode = DataMode::Passive;
                            e.owner = target;
                        }
                        info!(
                            "process={} transferred from {} to {}",
                            pid_str, actor_id, target
                        );
                        deliver(
                            &target_handle,
                            "handle-process-transfer",
                            Value::String(pid_str.clone()),
                            &pid_str,
                        )
                        .await;
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
