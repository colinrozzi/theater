//! # FileSystem Handler
//!
//! Sandboxed, permission-gated filesystem access for WebAssembly actors in the
//! Theater system. Implements the `theater:simple/filesystem` interface
//! (`filesystem.pact`): read-file / write-file / append-file / delete-file /
//! exists / list-dir / create-dir / remove-dir / metadata, plus the durability
//! ops sync-file / sync-dir / rename / write-file-durable.
//!
//! ## Security model
//!
//! - **Sandbox root.** Every actor-supplied path is resolved *relative to* the
//!   handler's configured sandbox root ([`FileSystemHandlerConfig::path`]). A
//!   path that escapes the root — via `..`, an absolute path, or a windows
//!   prefix — is rejected as `invalid-path`. When `new_dir` is set (or no root
//!   is configured) a fresh per-instance temp directory is used as the root, so
//!   an unconfigured handler never exposes the host filesystem.
//! - **Capability gate (default-deny).** Reads (read-file / exists / list-dir /
//!   metadata) require [`FileSystemPermissions::read`]; writes (write-file /
//!   append-file / delete-file / create-dir / remove-dir, and the durability
//!   ops) require
//!   [`FileSystemPermissions::write`]. A missing capability (or a false bit)
//!   yields `permission-denied`.
//! - **allowed-paths.** When [`FileSystemPermissions::allowed_paths`] is set, the
//!   resolved path must fall within one of those subtrees (interpreted relative
//!   to the sandbox root) or the op is rejected `permission-denied`.
//! - **rename.** Both paths pass the checks above, and additionally the real
//!   location of each path's parent directory (symlinks resolved) must be in
//!   the sandbox and the allowed-paths, so a rename can't move a file into or
//!   out of a place the actor can't write. See [`prepare_rename`].
//!
//! ## Sandbox-root lifecycle
//!
//! When the root is a fresh per-instance temp dir (`new_dir`, or no configured
//! `path`), the handler records it and removes it when the actor shuts down
//! (`setup`). It is *not* removed for a configured `path` — that belongs to the
//! operator. As a backstop for roots orphaned by a crash/SIGKILL (where
//! shutdown never fires), a one-time startup sweep removes `theater-fs-<pid>-*`
//! dirs whose owning process is no longer alive. Without this, every actor
//! instantiation leaked a temp dir — a slow disk + inode bomb on long-lived
//! hosts.
//!
//! ## Replay
//!
//! Every op here is a host call, so its result is recorded on the chain and
//! replayed deterministically — exactly like the store handler.
//!
//! ## Deferred
//!
//! Command execution (the `execute` permission bit and the
//! `allowed_commands` config field) is intentionally **not** implemented — this
//! is a filesystem-only v1. Those fields are accepted and ignored.

use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;

use anyhow::Context;
use thiserror::Error;
use tracing::{debug, info, warn};

use theater::actor::handle::ActorHandle;
use theater::config::actor_manifest::FileSystemHandlerConfig;
use theater::config::permissions::{FileSystemPermissions, HandlerPermission};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    pact_result_host_fn, parse_pact, InterfaceImpl, TypeHash, Value, ValueType,
};

/// Embedded filesystem.pact file content.
const FILESYSTEM_PACT: &str = include_str!("../filesystem.pact");

/// Declare the `theater:simple/filesystem` interface from the pact file.
fn filesystem_interface() -> InterfaceImpl {
    let pact = parse_pact(FILESYSTEM_PACT).expect("embedded filesystem.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Errors → the `filesystem-error` pact variant
// ============================================================================

/// The host-side mirror of the pact `filesystem-error` variant. [`Self::to_value`]
/// builds the wire `Value::Variant` with the tag matching the pact declaration
/// order.
#[derive(Debug, Error)]
enum FsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("not a directory: {0}")]
    NotADirectory(String),
    #[error("is a directory: {0}")]
    IsADirectory(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("io error: {0}")]
    IoError(String),
}

impl FsError {
    /// Build the `filesystem-error` pact variant. Tags match the declaration
    /// order in `filesystem.pact`.
    fn to_value(&self) -> Value {
        let (tag, case, msg) = match self {
            FsError::NotFound(m) => (0usize, "not-found", m),
            FsError::PermissionDenied(m) => (1, "permission-denied", m),
            FsError::AlreadyExists(m) => (2, "already-exists", m),
            FsError::NotADirectory(m) => (3, "not-a-directory", m),
            FsError::IsADirectory(m) => (4, "is-a-directory", m),
            FsError::InvalidPath(m) => (5, "invalid-path", m),
            FsError::IoError(m) => (6, "io-error", m),
        };
        Value::Variant {
            type_name: "filesystem-error".to_string(),
            case_name: case.to_string(),
            tag,
            payload: vec![Value::String(msg.clone())],
        }
    }
}

/// Map a host `io::Error` onto a `filesystem-error`. NOTE: a host-level
/// `PermissionDenied` maps to `io-error`, NOT the pact `permission-denied` — the
/// latter is reserved for OUR capability / sandbox checks.
fn map_io_error(e: std::io::Error, path: &str) -> FsError {
    let msg = format!("{}: {}", path, e);
    match e.kind() {
        std::io::ErrorKind::NotFound => FsError::NotFound(msg),
        std::io::ErrorKind::AlreadyExists => FsError::AlreadyExists(msg),
        std::io::ErrorKind::NotADirectory => FsError::NotADirectory(msg),
        std::io::ErrorKind::IsADirectory => FsError::IsADirectory(msg),
        // host PermissionDenied and everything else → io-error (detail preserved)
        _ => FsError::IoError(msg),
    }
}

// ============================================================================
// The security boundary: sandbox path resolution
// ============================================================================

/// Resolve an actor-supplied `user_path` inside the sandbox `root`.
///
/// `root` is expected to be a canonicalized, absolute directory. The user path
/// is treated as strictly relative: any `..` component, an absolute path, or a
/// filesystem prefix (windows drive / UNC) is rejected as `invalid-path`. A
/// defensive `starts_with(root)` check backstops the component walk.
///
/// This is intentionally pure path arithmetic (no disk access) so it is cheap
/// and unit-testable; the canonicalized root plus the no-`..` rule keeps a
/// resolved path under the root. (A symlink *inside* the sandbox that points out
/// is out of scope for v1 — see the pact note.)
fn resolve_in_sandbox(root: &Path, user_path: &str) -> Result<PathBuf, FsError> {
    let mut out = root.to_path_buf();
    for comp in Path::new(user_path).components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(FsError::InvalidPath(format!(
                    "path '{}' escapes the sandbox root via '..'",
                    user_path
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(FsError::InvalidPath(format!(
                    "path '{}' must be relative to the sandbox root",
                    user_path
                )));
            }
        }
    }
    if !out.starts_with(root) {
        return Err(FsError::InvalidPath(format!(
            "path '{}' escapes the sandbox root",
            user_path
        )));
    }
    Ok(out)
}

/// Enforce the read/write capability (default-deny) and return the granted
/// permission set for any further (allowed-paths) checks.
fn gate(
    perms: &Option<FileSystemPermissions>,
    write: bool,
) -> Result<&FileSystemPermissions, FsError> {
    let p = perms.as_ref().ok_or_else(|| {
        FsError::PermissionDenied("filesystem capability not granted".to_string())
    })?;
    let granted = if write { p.write } else { p.read };
    if granted {
        Ok(p)
    } else {
        Err(FsError::PermissionDenied(format!(
            "filesystem '{}' capability not granted",
            if write { "write" } else { "read" }
        )))
    }
}

/// When `allowed_paths` is set, require the resolved path to fall within one of
/// the listed subtrees. Each entry is interpreted **relative to the sandbox
/// root** — the sandbox root is already the outer boundary, so an allowlist just
/// narrows within it. A leading `/` is tolerated (the entry is still matched
/// root-relative), so a bare `"/"` means "the sandbox root" = allow anywhere in
/// the sandbox. This is what makes the common `theater spawn` grant
/// (`allowed_paths = ["/"]`, i.e. "all paths") resolve to "all of my sandbox"
/// instead of matching nothing.
fn check_allowed(
    root: &Path,
    perms: &FileSystemPermissions,
    candidate: &Path,
    user_path: &str,
) -> Result<(), FsError> {
    if let Some(allowed) = &perms.allowed_paths {
        let ok = allowed.iter().any(|a| {
            // Interpret the entry root-relative; a leading '/' (or a bare '/')
            // collapses to the root itself rather than being rejected as an
            // absolute escape by `resolve_in_sandbox`.
            let rel = a.trim_start_matches('/');
            let allowed_prefix = if rel.is_empty() {
                Some(root.to_path_buf())
            } else {
                resolve_in_sandbox(root, rel).ok()
            };
            allowed_prefix
                .map(|abs| candidate.starts_with(&abs))
                .unwrap_or(false)
        });
        if !ok {
            return Err(FsError::PermissionDenied(format!(
                "path '{}' is outside the allowed-paths",
                user_path
            )));
        }
    }
    Ok(())
}

/// Gate the capability, resolve the path inside the sandbox, and apply the
/// allowed-paths restriction — the full pre-flight every op shares.
fn prepare(
    perms: &Option<FileSystemPermissions>,
    root: &Path,
    user_path: &str,
    write: bool,
) -> Result<PathBuf, FsError> {
    let p = gate(perms, write)?;
    let resolved = resolve_in_sandbox(root, user_path)?;
    check_allowed(root, p, &resolved, user_path)?;
    Ok(resolved)
}

/// [`prepare`] for a write that renames: additionally resolve symlinks in the
/// path's *parent* directories and require the real location to be inside the
/// sandbox and the allowed-paths.
///
/// [`resolve_in_sandbox`] is pure path arithmetic, so a symlinked directory
/// inside the sandbox that points out (`out -> /elsewhere`) passes it. For most
/// ops that is a known v1 gap; for rename it would let an actor move a host file
/// into its sandbox, or move one of its files out. The final component is left
/// unresolved on purpose: rename acts on a symlink itself, never its target.
/// The sandbox root itself cannot be renamed or replaced.
async fn prepare_rename(
    perms: &Option<FileSystemPermissions>,
    root: &Path,
    user_path: &str,
) -> Result<PathBuf, FsError> {
    let p = gate(perms, true)?;
    let resolved = resolve_in_sandbox(root, user_path)?;
    check_allowed(root, p, &resolved, user_path)?;
    let (parent, name) = match (resolved.parent(), resolved.file_name()) {
        (Some(parent), Some(name)) if resolved != root => (parent, name),
        _ => {
            return Err(FsError::InvalidPath(format!(
                "path '{}' is the sandbox root",
                user_path
            )))
        }
    };
    let real = tokio::fs::canonicalize(parent)
        .await
        .map_err(|e| map_io_error(e, user_path))?
        .join(name);
    if !real.starts_with(root) {
        return Err(FsError::InvalidPath(format!(
            "path '{}' escapes the sandbox root via a symlink",
            user_path
        )));
    }
    check_allowed(root, p, &real, user_path)?;
    Ok(real)
}

// ============================================================================
// Durability ops (sync-file / sync-dir / rename / write-file-durable)
// ============================================================================

/// fsync an open file's data and metadata, requiring it to be a file
/// (`want_dir = false`) or a directory (`want_dir = true`).
async fn sync_path(path: &Path, want_dir: bool) -> std::io::Result<()> {
    let f = tokio::fs::File::open(path).await?;
    let is_dir = f.metadata().await?.is_dir();
    if is_dir != want_dir {
        return Err(std::io::Error::from(if want_dir {
            std::io::ErrorKind::NotADirectory
        } else {
            std::io::ErrorKind::IsADirectory
        }));
    }
    f.sync_all().await
}

async fn sync_file_op(
    perms: &Option<FileSystemPermissions>,
    root: &Path,
    path: &str,
) -> Result<(), FsError> {
    let resolved = prepare(perms, root, path, true)?;
    sync_path(&resolved, false)
        .await
        .map_err(|e| map_io_error(e, path))
}

async fn sync_dir_op(
    perms: &Option<FileSystemPermissions>,
    root: &Path,
    path: &str,
) -> Result<(), FsError> {
    let resolved = prepare(perms, root, path, true)?;
    sync_path(&resolved, true)
        .await
        .map_err(|e| map_io_error(e, path))
}

/// rename(2): atomic, replaces an existing file at `to`. Across filesystems
/// (a mount inside the sandbox) the OS returns EXDEV, which surfaces as
/// `io-error`; we deliberately do not fall back to copy + delete, which would
/// not be atomic.
async fn rename_op(
    perms: &Option<FileSystemPermissions>,
    root: &Path,
    from: &str,
    to: &str,
) -> Result<(), FsError> {
    let src = prepare_rename(perms, root, from).await?;
    let dst = prepare_rename(perms, root, to).await?;
    tokio::fs::rename(&src, &dst)
        .await
        .map_err(|e| map_io_error(e, &format!("{} -> {}", from, to)))
}

/// Write a hidden temp file next to `path`, sync it, rename it over `path`,
/// then sync the directory. The temp file is removed if any step before the
/// rename fails.
async fn write_file_durable_op(
    perms: &Option<FileSystemPermissions>,
    root: &Path,
    path: &str,
    content: &[u8],
) -> Result<(), FsError> {
    use tokio::io::AsyncWriteExt;
    let target = prepare_rename(perms, root, path).await?;
    // prepare_rename guarantees a parent and a file name.
    let dir = target.parent().expect("target has a parent");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        target
            .file_name()
            .expect("target has a name")
            .to_string_lossy(),
        nanos
    ));
    let res = async {
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .await?;
        f.write_all(content).await?;
        f.flush().await?;
        f.sync_all().await?;
        drop(f);
        tokio::fs::rename(&tmp, &target).await
    }
    .await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(map_io_error(e, path));
    }
    sync_path(dir, true)
        .await
        .map_err(|e| map_io_error(e, path))
}

// ============================================================================
// Input parsing (Composite Value → args)
// ============================================================================

/// A single `path: string` argument. Accepts a bare `String` or a 1-tuple, to
/// match how the guest marshals single-argument calls (mirrors the store handler).
fn parse_path(input: &Value) -> Result<String, FsError> {
    match input {
        Value::String(s) => Ok(s.clone()),
        Value::Tuple(fields) if fields.len() == 1 => match &fields[0] {
            Value::String(s) => Ok(s.clone()),
            _ => Err(FsError::InvalidPath("expected string for path".to_string())),
        },
        _ => Err(FsError::InvalidPath("expected string for path".to_string())),
    }
}

/// A `(from: string, to: string)` argument tuple.
fn parse_two_paths(input: &Value) -> Result<(String, String), FsError> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => match (&fields[0], &fields[1]) {
            (Value::String(a), Value::String(b)) => Ok((a.clone(), b.clone())),
            _ => Err(FsError::InvalidPath(
                "expected strings for paths".to_string(),
            )),
        },
        _ => Err(FsError::InvalidPath(
            "expected tuple (from, to)".to_string(),
        )),
    }
}

/// A `(path: string, content: list<u8>)` argument tuple.
fn parse_path_and_content(input: &Value) -> Result<(String, Vec<u8>), FsError> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let path = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(FsError::InvalidPath("expected string for path".to_string())),
            };
            let content = match &fields[1] {
                Value::List { items, .. } => items
                    .iter()
                    .filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    })
                    .collect::<Vec<u8>>(),
                _ => {
                    return Err(FsError::InvalidPath(
                        "expected list<u8> for content".to_string(),
                    ))
                }
            };
            Ok((path, content))
        }
        _ => Err(FsError::InvalidPath(
            "expected tuple (path, content)".to_string(),
        )),
    }
}

// ============================================================================
// Handler
// ============================================================================

/// Handler exposing sandboxed filesystem access to WebAssembly actors.
#[derive(Clone)]
pub struct FileSystemHandler {
    permissions: Option<FileSystemPermissions>,
    /// Configured sandbox root. When `None`, a fresh per-instance temp directory
    /// is used (see [`Self::resolve_root`]).
    path: Option<PathBuf>,
    /// When `Some(true)`, always root the sandbox at a fresh temp directory.
    new_dir: Option<bool>,
    /// The per-instance temp directory this handler created as its sandbox root,
    /// if any. Set by [`Self::resolve_root`] when the root is a fresh temp dir
    /// (not a configured `path`); removed in [`Handler::setup`] when the actor
    /// shuts down. `None` for a configured (persistent) root — we must never
    /// delete a path the operator handed us.
    ephemeral_root: Option<PathBuf>,
}

impl FileSystemHandler {
    /// Create a new filesystem handler from the manifest config and the actor's
    /// granted permissions.
    ///
    /// NOTE: `config.allowed_commands` and the `execute` permission bit are
    /// intentionally ignored — command execution is DEFERRED (filesystem-only v1).
    pub fn new(
        config: FileSystemHandlerConfig,
        permissions: Option<FileSystemPermissions>,
    ) -> Self {
        Self {
            permissions,
            path: config.path,
            new_dir: config.new_dir,
            ephemeral_root: None,
        }
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![filesystem_interface()]
    }

    /// Resolve (and create + canonicalize) the sandbox root for this instance.
    ///
    /// - `new_dir = Some(true)` → a fresh per-instance temp directory.
    /// - otherwise a configured `path` is used.
    /// - no configured path (and no `new_dir`) → a fresh temp directory, so an
    ///   unconfigured handler never exposes the host filesystem.
    fn resolve_root(&mut self) -> anyhow::Result<PathBuf> {
        let (root, ephemeral) = match (&self.path, self.new_dir) {
            (_, Some(true)) => (fresh_temp_root(), true),
            (Some(p), _) => (p.clone(), false),
            (None, _) => (fresh_temp_root(), true),
        };
        std::fs::create_dir_all(&root)
            .with_context(|| format!("creating filesystem sandbox root {:?}", root))?;
        let canonical = std::fs::canonicalize(&root)
            .with_context(|| format!("canonicalizing filesystem sandbox root {:?}", root))?;
        if ephemeral {
            // Remember the per-instance temp root so `setup` can remove it when
            // the actor shuts down. Without this, every actor instantiation
            // leaks a `theater-fs-<pid>-<ts>` dir — a slow disk + inode bomb on
            // long-lived hosts (an acceptor instantiated 900K of these once).
            self.ephemeral_root = Some(canonical.clone());
        }
        Ok(canonical)
    }
}

/// A fresh, process-and-time-unique temp directory path (not yet created). The
/// `theater-fs-<pid>-<nanos>` shape is what [`parse_sandbox_pid`] recognizes
/// when sweeping orphans, so keep the two in sync.
fn fresh_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("theater-fs-{}-{}", std::process::id(), nanos))
}

/// Guards [`sweep_orphan_sandboxes`] so it runs at most once per process.
static ORPHAN_SWEEP: std::sync::Once = std::sync::Once::new();

/// Kick off the one-time orphan sweep (on a detached thread so a pathological
/// temp dir can't stall the first actor's startup). Idempotent across handlers
/// and actors.
fn sweep_orphans_once() {
    ORPHAN_SWEEP.call_once(|| {
        std::thread::spawn(sweep_orphan_sandboxes);
    });
}

/// Remove leftover `theater-fs-<pid>-<nanos>` sandbox roots whose owning process
/// is no longer alive — the orphans a crash/SIGKILL (or a pre-GC build) left
/// behind, which the per-actor shutdown GC can't reach. Dirs belonging to a
/// live process (including ours) are left untouched: ours are GC'd on shutdown,
/// and a reused pid is kept conservatively rather than risk deleting a running
/// host's sandbox.
fn sweep_orphan_sandboxes() {
    let tmp = std::env::temp_dir();
    let me = std::process::id();
    let rd = match std::fs::read_dir(&tmp) {
        Ok(rd) => rd,
        Err(e) => {
            warn!("theater-fs orphan sweep: cannot read {:?}: {}", tmp, e);
            return;
        }
    };
    let (mut removed, mut failed) = (0u64, 0u64);
    for entry in rd.flatten() {
        let name = entry.file_name();
        let pid = match name.to_str().and_then(parse_sandbox_pid) {
            Some(p) => p,
            None => continue,
        };
        if pid == me || is_pid_alive(pid) {
            continue;
        }
        match std::fs::remove_dir_all(entry.path()) {
            Ok(()) => removed += 1,
            Err(_) => failed += 1,
        }
    }
    if removed > 0 || failed > 0 {
        info!(
            "theater-fs orphan sweep: removed {} stale sandbox dir(s), {} failed",
            removed, failed
        );
    }
}

/// Parse the `<pid>` out of a `theater-fs-<pid>-<nanos>` dir name. Returns
/// `None` for anything that is not exactly that shape, so the sweep never
/// touches an unrelated temp dir.
fn parse_sandbox_pid(name: &str) -> Option<u32> {
    let (pid, nanos) = name.strip_prefix("theater-fs-")?.split_once('-')?;
    if nanos.is_empty() || !nanos.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid.parse::<u32>().ok()
}

/// Is `pid` a live process? Linux-only, via `/proc/<pid>`. Where `/proc` is
/// absent (non-Linux) we conservatively report `true` so the sweep can never
/// delete a live host's sandbox; the per-actor shutdown GC still runs there.
fn is_pid_alive(pid: u32) -> bool {
    let proc = Path::new("/proc");
    if !proc.exists() {
        return true;
    }
    proc.join(pid.to_string()).exists()
}

impl Handler for FileSystemHandler {
    fn create_instance(
        &self,
        config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        match config {
            Some(c) => match c.parse::<FileSystemHandlerConfig>() {
                Ok(fc) => Box::new(FileSystemHandler::new(fc, self.permissions.clone())),
                Err(e) => {
                    warn!(
                        "invalid filesystem handler config: {}; keeping template config",
                        e
                    );
                    Box::new(self.clone())
                }
            },
            None => Box::new(self.clone()),
        }
    }

    fn set_permissions(&mut self, permissions: Option<&HandlerPermission>) {
        // Bake in this actor's granted filesystem capability. `None` → default-deny.
        self.permissions = permissions.and_then(|p| p.file_system.clone());
    }

    fn setup(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("FileSystem handler setup");
        // Capture the per-instance temp root (if any) so we can delete it once
        // the actor stops — the fix for the sandbox-root leak.
        let ephemeral_root = self.ephemeral_root.clone();
        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("FileSystem handler received shutdown signal");
            if let Some(root) = ephemeral_root {
                match std::fs::remove_dir_all(&root) {
                    Ok(()) => debug!("removed ephemeral filesystem sandbox root {:?}", root),
                    Err(e) => warn!(
                        "failed to remove ephemeral filesystem sandbox root {:?}: {}",
                        root, e
                    ),
                }
            }
            Ok(())
        })
    }

    fn name(&self) -> &str {
        "filesystem"
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
        None
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![filesystem_interface()]
    }

    // =========================================================================
    // Composite Integration
    // =========================================================================

    fn register_host_functions(
        &mut self,
        imports: &mut theater::pack_bridge::HostImports,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up filesystem host functions (Pack)");

        // One-time per process: clear sandbox roots orphaned by dead processes.
        sweep_orphans_once();

        if ctx.is_satisfied("theater:simple/filesystem") {
            info!("theater:simple/filesystem already satisfied, skipping");
            return Ok(());
        }

        let root = self.resolve_root()?;
        debug!("filesystem sandbox root = {:?}", root);
        let perms = self.permissions.clone();

        // Per-op owned copies (each host fn is an independent `move` closure).
        let (r_read, p_read) = (root.clone(), perms.clone());
        let (r_exists, p_exists) = (root.clone(), perms.clone());
        let (r_list, p_list) = (root.clone(), perms.clone());
        let (r_meta, p_meta) = (root.clone(), perms.clone());
        let (r_write, p_write) = (root.clone(), perms.clone());
        let (r_append, p_append) = (root.clone(), perms.clone());
        let (r_delete, p_delete) = (root.clone(), perms.clone());
        let (r_cdir, p_cdir) = (root.clone(), perms.clone());
        let (r_rdir, p_rdir) = (root.clone(), perms.clone());
        let (r_sfile, p_sfile) = (root.clone(), perms.clone());
        let (r_sdir, p_sdir) = (root.clone(), perms.clone());
        let (r_rename, p_rename) = (root.clone(), perms.clone());
        let (r_durable, p_durable) = (root.clone(), perms.clone());

        // --- reads (require `read`) ---

        // read-file(path: string) -> result<list<u8>, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "read-file",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_read.clone(), p_read.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    let resolved =
                        prepare(&perms, &root, &path, false).map_err(|e| e.to_value())?;
                    match tokio::fs::read(&resolved).await {
                        Ok(bytes) => Ok(Value::List {
                            elem_type: ValueType::U8,
                            items: bytes.into_iter().map(Value::U8).collect(),
                        }),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // exists(path: string) -> result<bool, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "exists",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_exists.clone(), p_exists.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    let resolved =
                        prepare(&perms, &root, &path, false).map_err(|e| e.to_value())?;
                    match tokio::fs::try_exists(&resolved).await {
                        Ok(b) => Ok(Value::Bool(b)),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // list-dir(path: string) -> result<list<dir-entry>, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "list-dir",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_list.clone(), p_list.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    let resolved =
                        prepare(&perms, &root, &path, false).map_err(|e| e.to_value())?;
                    let mut rd = match tokio::fs::read_dir(&resolved).await {
                        Ok(rd) => rd,
                        Err(e) => return Err(map_io_error(e, &path).to_value()),
                    };
                    let mut items = Vec::new();
                    loop {
                        match rd.next_entry().await {
                            Ok(Some(entry)) => {
                                let name = entry.file_name().to_string_lossy().into_owned();
                                let is_dir = match entry.file_type().await {
                                    Ok(ft) => ft.is_dir(),
                                    Err(e) => return Err(map_io_error(e, &path).to_value()),
                                };
                                items.push(Value::Record {
                                    type_name: "dir-entry".to_string(),
                                    fields: vec![
                                        ("name".to_string(), Value::String(name)),
                                        ("is-dir".to_string(), Value::Bool(is_dir)),
                                    ],
                                });
                            }
                            Ok(None) => break,
                            Err(e) => return Err(map_io_error(e, &path).to_value()),
                        }
                    }
                    Ok(Value::List {
                        elem_type: ValueType::Record("dir-entry".to_string()),
                        items,
                    })
                }
            }),
        );

        // metadata(path: string) -> result<file-metadata, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "metadata",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_meta.clone(), p_meta.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    let resolved =
                        prepare(&perms, &root, &path, false).map_err(|e| e.to_value())?;
                    match tokio::fs::metadata(&resolved).await {
                        Ok(m) => Ok(Value::Record {
                            type_name: "file-metadata".to_string(),
                            fields: vec![
                                ("size".to_string(), Value::U64(m.len())),
                                ("is-dir".to_string(), Value::Bool(m.is_dir())),
                                (
                                    "read-only".to_string(),
                                    Value::Bool(m.permissions().readonly()),
                                ),
                            ],
                        }),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // --- writes (require `write`) ---

        // write-file(path: string, content: list<u8>) -> result<_, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "write-file",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_write.clone(), p_write.clone());
                async move {
                    let (path, content) =
                        parse_path_and_content(&input).map_err(|e| e.to_value())?;
                    let resolved = prepare(&perms, &root, &path, true).map_err(|e| e.to_value())?;
                    match tokio::fs::write(&resolved, &content).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // append-file(path: string, content: list<u8>) -> result<_, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "append-file",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_append.clone(), p_append.clone());
                async move {
                    use tokio::io::AsyncWriteExt;
                    let (path, content) =
                        parse_path_and_content(&input).map_err(|e| e.to_value())?;
                    let resolved = prepare(&perms, &root, &path, true).map_err(|e| e.to_value())?;
                    let res = async {
                        let mut f = tokio::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&resolved)
                            .await?;
                        f.write_all(&content).await?;
                        f.flush().await?;
                        Ok::<(), std::io::Error>(())
                    }
                    .await;
                    match res {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // delete-file(path: string) -> result<_, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "delete-file",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_delete.clone(), p_delete.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    let resolved = prepare(&perms, &root, &path, true).map_err(|e| e.to_value())?;
                    match tokio::fs::remove_file(&resolved).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // create-dir(path: string) -> result<_, filesystem-error>  (mkdir -p)
        imports.define(
            "theater:simple/filesystem",
            "create-dir",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_cdir.clone(), p_cdir.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    let resolved = prepare(&perms, &root, &path, true).map_err(|e| e.to_value())?;
                    match tokio::fs::create_dir_all(&resolved).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // remove-dir(path: string) -> result<_, filesystem-error>  (recursive)
        imports.define(
            "theater:simple/filesystem",
            "remove-dir",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_rdir.clone(), p_rdir.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    let resolved = prepare(&perms, &root, &path, true).map_err(|e| e.to_value())?;
                    match tokio::fs::remove_dir_all(&resolved).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(map_io_error(e, &path).to_value()),
                    }
                }
            }),
        );

        // --- durability (require `write`) ---

        // sync-file(path: string) -> result<_, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "sync-file",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_sfile.clone(), p_sfile.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    match sync_file_op(&perms, &root, &path).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(e.to_value()),
                    }
                }
            }),
        );

        // sync-dir(path: string) -> result<_, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "sync-dir",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_sdir.clone(), p_sdir.clone());
                async move {
                    let path = parse_path(&input).map_err(|e| e.to_value())?;
                    match sync_dir_op(&perms, &root, &path).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(e.to_value()),
                    }
                }
            }),
        );

        // rename(from: string, to: string) -> result<_, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "rename",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_rename.clone(), p_rename.clone());
                async move {
                    let (from, to) = parse_two_paths(&input).map_err(|e| e.to_value())?;
                    match rename_op(&perms, &root, &from, &to).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(e.to_value()),
                    }
                }
            }),
        );

        // write-file-durable(path: string, content: list<u8>) -> result<_, filesystem-error>
        imports.define(
            "theater:simple/filesystem",
            "write-file-durable",
            pact_result_host_fn(move |input: Value| {
                let (root, perms) = (r_durable.clone(), p_durable.clone());
                async move {
                    let (path, content) =
                        parse_path_and_content(&input).map_err(|e| e.to_value())?;
                    match write_file_durable_op(&perms, &root, &path, &content).await {
                        Ok(()) => Ok(Value::Tuple(vec![])),
                        Err(e) => Err(e.to_value()),
                    }
                }
            }),
        );

        ctx.mark_satisfied("theater:simple/filesystem");
        info!("Filesystem host functions (Pack) set up successfully");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> FileSystemHandlerConfig {
        FileSystemHandlerConfig {
            path: None,
            new_dir: None,
            allowed_commands: None,
        }
    }

    #[test]
    fn test_filesystem_handler_creation() {
        let handler = FileSystemHandler::new(cfg(), None);
        assert_eq!(handler.name(), "filesystem");
        assert_eq!(
            handler.imports(),
            Some(vec!["theater:simple/filesystem".to_string()])
        );
        assert_eq!(handler.exports(), None);
    }

    #[test]
    fn test_filesystem_handler_clone() {
        let handler = FileSystemHandler::new(cfg(), None);
        let cloned = handler.create_instance(None);
        assert_eq!(cloned.name(), "filesystem");
    }

    #[test]
    fn test_interface_hash_determinism() {
        assert_eq!(filesystem_interface().hash(), filesystem_interface().hash());
    }

    #[test]
    fn test_interface_hashes_nonzero() {
        let handler = FileSystemHandler::new(cfg(), None);
        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/filesystem");
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }

    // --- resolve_in_sandbox: THE security boundary ---

    #[test]
    fn test_sandbox_normal_path_ok() {
        let root = Path::new("/sandbox");
        let resolved = resolve_in_sandbox(root, "a/b.txt").expect("normal path should resolve");
        assert_eq!(resolved, PathBuf::from("/sandbox/a/b.txt"));
        assert!(resolved.starts_with(root));
    }

    #[test]
    fn test_sandbox_current_dir_segments_ok() {
        let root = Path::new("/sandbox");
        let resolved = resolve_in_sandbox(root, "./sub/./f").expect("curdir segments ok");
        assert_eq!(resolved, PathBuf::from("/sandbox/sub/f"));
    }

    #[test]
    fn test_sandbox_empty_path_is_root() {
        let root = Path::new("/sandbox");
        let resolved = resolve_in_sandbox(root, "").expect("empty path resolves to root");
        assert_eq!(resolved, PathBuf::from("/sandbox"));
    }

    #[test]
    fn test_sandbox_traversal_rejected() {
        let root = Path::new("/sandbox");
        assert!(matches!(
            resolve_in_sandbox(root, "../etc/passwd"),
            Err(FsError::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_in_sandbox(root, "a/../../b"),
            Err(FsError::InvalidPath(_))
        ));
    }

    #[test]
    fn test_sandbox_absolute_rejected() {
        let root = Path::new("/sandbox");
        assert!(matches!(
            resolve_in_sandbox(root, "/etc/passwd"),
            Err(FsError::InvalidPath(_))
        ));
    }

    // --- permission gate: default-deny ---

    #[test]
    fn test_gate_default_deny_when_none() {
        assert!(matches!(
            gate(&None, false),
            Err(FsError::PermissionDenied(_))
        ));
        assert!(matches!(
            gate(&None, true),
            Err(FsError::PermissionDenied(_))
        ));
    }

    #[test]
    fn test_gate_read_write_bits() {
        let read_only = Some(FileSystemPermissions {
            read: true,
            write: false,
            execute: false,
            allowed_commands: None,
            new_dir: None,
            allowed_paths: None,
        });
        assert!(gate(&read_only, false).is_ok());
        assert!(matches!(
            gate(&read_only, true),
            Err(FsError::PermissionDenied(_))
        ));
    }

    #[test]
    fn test_allowed_paths_restriction() {
        let root = Path::new("/sandbox");
        let perms = FileSystemPermissions {
            read: true,
            write: false,
            execute: false,
            allowed_commands: None,
            new_dir: None,
            allowed_paths: Some(vec!["public".to_string()]),
        };
        let inside = resolve_in_sandbox(root, "public/f.txt").unwrap();
        assert!(check_allowed(root, &perms, &inside, "public/f.txt").is_ok());
        let outside = resolve_in_sandbox(root, "private/f.txt").unwrap();
        assert!(matches!(
            check_allowed(root, &perms, &outside, "private/f.txt"),
            Err(FsError::PermissionDenied(_))
        ));
    }

    #[test]
    fn test_allowed_paths_root_slash_permits_whole_sandbox() {
        // The `theater spawn` root grant is allowed_paths = ["/"] ("all paths").
        // A bare "/" must resolve to the sandbox root and permit any in-sandbox
        // path, not match nothing. (Regression: absolute "/" was rejected by
        // resolve_in_sandbox, wedging every fs op for a top-level actor.)
        let root = Path::new("/sandbox");
        let perms = FileSystemPermissions {
            read: true,
            write: true,
            execute: false,
            allowed_commands: None,
            new_dir: None,
            allowed_paths: Some(vec!["/".to_string()]),
        };
        let a = resolve_in_sandbox(root, "crasher.jsonl").unwrap();
        assert!(check_allowed(root, &perms, &a, "crasher.jsonl").is_ok());
        let b = resolve_in_sandbox(root, "sub/dir/f.txt").unwrap();
        assert!(check_allowed(root, &perms, &b, "sub/dir/f.txt").is_ok());
    }

    #[test]
    fn test_fs_error_variant_tags() {
        // Tags must match filesystem.pact declaration order.
        let cases = [
            (FsError::NotFound(String::new()), 0usize, "not-found"),
            (
                FsError::PermissionDenied(String::new()),
                1,
                "permission-denied",
            ),
            (FsError::AlreadyExists(String::new()), 2, "already-exists"),
            (FsError::NotADirectory(String::new()), 3, "not-a-directory"),
            (FsError::IsADirectory(String::new()), 4, "is-a-directory"),
            (FsError::InvalidPath(String::new()), 5, "invalid-path"),
            (FsError::IoError(String::new()), 6, "io-error"),
        ];
        for (err, tag, case) in cases {
            match err.to_value() {
                Value::Variant {
                    type_name,
                    case_name,
                    tag: t,
                    ..
                } => {
                    assert_eq!(type_name, "filesystem-error");
                    assert_eq!(case_name, case);
                    assert_eq!(t, tag);
                }
                other => panic!("expected variant, got {:?}", other),
            }
        }
    }

    // --- sandbox-root GC (the /tmp leak fix) ---

    #[test]
    fn test_parse_sandbox_pid_shape() {
        // Exact shape → the pid.
        assert_eq!(parse_sandbox_pid("theater-fs-1234-56789"), Some(1234));
        assert_eq!(parse_sandbox_pid("theater-fs-1-0"), Some(1));
        // Wrong prefix, missing halves, or non-numeric nanos → None (never swept).
        assert_eq!(parse_sandbox_pid("theater-fs-1234"), None);
        assert_eq!(parse_sandbox_pid("theater-fs--5"), None);
        assert_eq!(parse_sandbox_pid("theater-fs-12-ab"), None);
        assert_eq!(parse_sandbox_pid("not-ours-1-2"), None);
        assert_eq!(parse_sandbox_pid("theater-fs-"), None);
    }

    #[test]
    fn test_fresh_temp_root_name_is_parseable() {
        // The name fresh_temp_root() mints must be recognized by the sweeper,
        // and must carry our own pid.
        let root = fresh_temp_root();
        let name = root.file_name().unwrap().to_str().unwrap();
        assert_eq!(parse_sandbox_pid(name), Some(std::process::id()));
    }

    #[test]
    fn test_ephemeral_root_recorded_and_removable() {
        // A no-config handler roots at a fresh temp dir, records it, and that
        // dir is what setup() would remove on shutdown.
        let mut handler = FileSystemHandler::new(cfg(), None);
        assert!(handler.ephemeral_root.is_none());
        let root = handler.resolve_root().expect("resolve a fresh root");
        assert!(root.exists());
        assert_eq!(handler.ephemeral_root.as_deref(), Some(root.as_path()));
        // Simulate the shutdown GC.
        std::fs::remove_dir_all(&root).expect("remove ephemeral root");
        assert!(!root.exists());
    }

    #[test]
    fn test_configured_root_is_not_ephemeral() {
        // A configured path must never be recorded as ephemeral — we must not
        // delete an operator-provided directory on shutdown.
        let dir = std::env::temp_dir().join(fresh_temp_root().file_name().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = FileSystemHandlerConfig {
            path: Some(dir.clone()),
            new_dir: None,
            allowed_commands: None,
        };
        let mut handler = FileSystemHandler::new(cfg, None);
        let root = handler.resolve_root().expect("resolve configured root");
        assert_eq!(root, std::fs::canonicalize(&dir).unwrap());
        assert!(handler.ephemeral_root.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- durability ops: sync-file / sync-dir / rename / write-file-durable ---

    /// A real, canonicalized scratch directory, removed on drop. `sandbox` is
    /// the sandbox root; `outside` is a sibling the actor must never reach.
    struct Scratch {
        base: PathBuf,
        sandbox: PathBuf,
        outside: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "theater-fs-test-{}",
                fresh_temp_root().file_name().unwrap().to_string_lossy()
            ));
            std::fs::create_dir_all(base.join("sandbox")).unwrap();
            std::fs::create_dir_all(base.join("outside")).unwrap();
            let base = std::fs::canonicalize(&base).unwrap();
            Scratch {
                sandbox: base.join("sandbox"),
                outside: base.join("outside"),
                base,
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.base).ok();
        }
    }

    fn perms(write: bool, allowed_paths: Option<Vec<&str>>) -> Option<FileSystemPermissions> {
        Some(FileSystemPermissions {
            read: true,
            write,
            execute: false,
            allowed_commands: None,
            new_dir: None,
            allowed_paths: allowed_paths.map(|v| v.into_iter().map(String::from).collect()),
        })
    }

    #[tokio::test]
    async fn test_sync_file_and_dir() {
        let s = Scratch::new();
        let rw = perms(true, None);
        std::fs::create_dir(s.sandbox.join("d")).unwrap();
        std::fs::write(s.sandbox.join("d/f"), b"x").unwrap();

        assert!(sync_file_op(&rw, &s.sandbox, "d/f").await.is_ok());
        assert!(sync_dir_op(&rw, &s.sandbox, "d").await.is_ok());
        assert!(sync_dir_op(&rw, &s.sandbox, "").await.is_ok());

        assert!(matches!(
            sync_file_op(&rw, &s.sandbox, "d").await,
            Err(FsError::IsADirectory(_))
        ));
        assert!(matches!(
            sync_dir_op(&rw, &s.sandbox, "d/f").await,
            Err(FsError::NotADirectory(_))
        ));
        assert!(matches!(
            sync_file_op(&rw, &s.sandbox, "missing").await,
            Err(FsError::NotFound(_))
        ));
        assert!(matches!(
            sync_file_op(&perms(false, None), &s.sandbox, "d/f").await,
            Err(FsError::PermissionDenied(_))
        ));
        assert!(matches!(
            sync_dir_op(&rw, &s.sandbox, "../outside").await,
            Err(FsError::InvalidPath(_))
        ));
    }

    #[tokio::test]
    async fn test_rename_moves_and_replaces() {
        let s = Scratch::new();
        let rw = perms(true, None);
        std::fs::write(s.sandbox.join("a"), b"new").unwrap();
        std::fs::write(s.sandbox.join("b"), b"old").unwrap();

        rename_op(&rw, &s.sandbox, "a", "b").await.unwrap();
        assert!(!s.sandbox.join("a").exists());
        assert_eq!(std::fs::read(s.sandbox.join("b")).unwrap(), b"new");

        std::fs::create_dir(s.sandbox.join("sub")).unwrap();
        rename_op(&rw, &s.sandbox, "b", "sub/c").await.unwrap();
        assert_eq!(std::fs::read(s.sandbox.join("sub/c")).unwrap(), b"new");

        assert!(matches!(
            rename_op(&rw, &s.sandbox, "missing", "x").await,
            Err(FsError::NotFound(_))
        ));
        assert!(matches!(
            rename_op(&rw, &s.sandbox, "sub/c", "nodir/c").await,
            Err(FsError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_rename_rejects_paths_outside_the_sandbox() {
        let s = Scratch::new();
        let rw = perms(true, None);
        std::fs::write(s.sandbox.join("in"), b"in").unwrap();
        std::fs::write(s.outside.join("secret"), b"secret").unwrap();

        for (from, to) in [
            ("in", "../outside/in"),
            ("../outside/secret", "stolen"),
            ("in", "a/../../outside/in"),
            ("a/../../outside/secret", "stolen"),
        ] {
            assert!(
                matches!(
                    rename_op(&rw, &s.sandbox, from, to).await,
                    Err(FsError::InvalidPath(_))
                ),
                "{} -> {}",
                from,
                to
            );
        }
        let abs_secret = s.outside.join("secret").to_string_lossy().into_owned();
        let abs_dst = s.outside.join("in").to_string_lossy().into_owned();
        assert!(matches!(
            rename_op(&rw, &s.sandbox, &abs_secret, "stolen").await,
            Err(FsError::InvalidPath(_))
        ));
        assert!(matches!(
            rename_op(&rw, &s.sandbox, "in", &abs_dst).await,
            Err(FsError::InvalidPath(_))
        ));
        // The sandbox root itself can be neither source nor target.
        assert!(matches!(
            rename_op(&rw, &s.sandbox, "", "x").await,
            Err(FsError::InvalidPath(_))
        ));
        assert!(matches!(
            rename_op(&rw, &s.sandbox, "in", ".").await,
            Err(FsError::InvalidPath(_))
        ));

        assert!(s.sandbox.join("in").exists());
        assert!(s.outside.join("secret").exists());
        assert!(!s.outside.join("in").exists());
        assert!(!s.sandbox.join("stolen").exists());
    }

    #[tokio::test]
    async fn test_rename_requires_write_on_both_paths() {
        let s = Scratch::new();
        std::fs::create_dir(s.sandbox.join("public")).unwrap();
        std::fs::create_dir(s.sandbox.join("private")).unwrap();
        std::fs::write(s.sandbox.join("public/f"), b"f").unwrap();
        std::fs::write(s.sandbox.join("private/g"), b"g").unwrap();
        let public_only = perms(true, Some(vec!["public"]));

        assert!(matches!(
            rename_op(&public_only, &s.sandbox, "public/f", "private/f").await,
            Err(FsError::PermissionDenied(_))
        ));
        assert!(matches!(
            rename_op(&public_only, &s.sandbox, "private/g", "public/g").await,
            Err(FsError::PermissionDenied(_))
        ));
        assert!(matches!(
            rename_op(&perms(false, None), &s.sandbox, "public/f", "public/h").await,
            Err(FsError::PermissionDenied(_))
        ));
        assert!(matches!(
            rename_op(&None, &s.sandbox, "public/f", "public/h").await,
            Err(FsError::PermissionDenied(_))
        ));
        rename_op(&public_only, &s.sandbox, "public/f", "public/h")
            .await
            .unwrap();
        assert!(s.sandbox.join("private/g").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_rename_symlinked_dir_cannot_escape() {
        // `out` is a symlink inside the sandbox to a directory outside it.
        // Path arithmetic alone sees `out/secret` as in-sandbox; rename must
        // resolve the parent and refuse both directions.
        let s = Scratch::new();
        let rw = perms(true, None);
        std::os::unix::fs::symlink(&s.outside, s.sandbox.join("out")).unwrap();
        std::fs::write(s.outside.join("secret"), b"secret").unwrap();
        std::fs::write(s.sandbox.join("in"), b"in").unwrap();

        assert!(matches!(
            rename_op(&rw, &s.sandbox, "out/secret", "stolen").await,
            Err(FsError::InvalidPath(_))
        ));
        assert!(matches!(
            rename_op(&rw, &s.sandbox, "in", "out/in").await,
            Err(FsError::InvalidPath(_))
        ));
        assert!(matches!(
            write_file_durable_op(&rw, &s.sandbox, "out/new", b"x").await,
            Err(FsError::InvalidPath(_))
        ));
        assert!(s.outside.join("secret").exists());
        assert!(!s.outside.join("in").exists());
        assert!(!s.outside.join("new").exists());
        assert!(s.sandbox.join("in").exists());

        // A symlinked dir that stays inside the sandbox is fine, and an
        // allowed-paths check applies to where it really points.
        std::fs::create_dir(s.sandbox.join("private")).unwrap();
        std::fs::create_dir(s.sandbox.join("public")).unwrap();
        std::os::unix::fs::symlink(s.sandbox.join("private"), s.sandbox.join("public/p")).unwrap();
        std::fs::write(s.sandbox.join("public/f"), b"f").unwrap();
        assert!(matches!(
            rename_op(
                &perms(true, Some(vec!["public"])),
                &s.sandbox,
                "public/f",
                "public/p/f"
            )
            .await,
            Err(FsError::PermissionDenied(_))
        ));
        rename_op(&rw, &s.sandbox, "public/f", "public/p/f")
            .await
            .unwrap();
        assert!(s.sandbox.join("private/f").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_rename_acts_on_the_symlink_not_its_target() {
        let s = Scratch::new();
        let rw = perms(true, None);
        std::fs::write(s.outside.join("secret"), b"secret").unwrap();
        std::os::unix::fs::symlink(s.outside.join("secret"), s.sandbox.join("link")).unwrap();

        // Moving the link moves only the link.
        rename_op(&rw, &s.sandbox, "link", "moved").await.unwrap();
        assert!(s
            .sandbox
            .join("moved")
            .symlink_metadata()
            .unwrap()
            .is_symlink());
        assert_eq!(std::fs::read(s.outside.join("secret")).unwrap(), b"secret");

        // Renaming over the link replaces the link, not the file it points to.
        std::fs::write(s.sandbox.join("f"), b"mine").unwrap();
        rename_op(&rw, &s.sandbox, "f", "moved").await.unwrap();
        assert!(!s
            .sandbox
            .join("moved")
            .symlink_metadata()
            .unwrap()
            .is_symlink());
        assert_eq!(std::fs::read(s.sandbox.join("moved")).unwrap(), b"mine");
        assert_eq!(std::fs::read(s.outside.join("secret")).unwrap(), b"secret");
    }

    #[tokio::test]
    async fn test_write_file_durable() {
        let s = Scratch::new();
        let rw = perms(true, None);
        std::fs::create_dir(s.sandbox.join("d")).unwrap();

        write_file_durable_op(&rw, &s.sandbox, "d/f", b"one")
            .await
            .unwrap();
        assert_eq!(std::fs::read(s.sandbox.join("d/f")).unwrap(), b"one");
        write_file_durable_op(&rw, &s.sandbox, "d/f", b"two")
            .await
            .unwrap();
        assert_eq!(std::fs::read(s.sandbox.join("d/f")).unwrap(), b"two");
        // No temp file left behind.
        let names: Vec<_> = std::fs::read_dir(s.sandbox.join("d"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("f")]);

        // Failure before the rename leaves the target untouched and no temp.
        assert!(matches!(
            write_file_durable_op(&rw, &s.sandbox, "nodir/f", b"x").await,
            Err(FsError::NotFound(_))
        ));
        assert!(matches!(
            write_file_durable_op(&rw, &s.sandbox, "d", b"x").await,
            Err(FsError::IsADirectory(_)) | Err(FsError::IoError(_))
        ));
        assert_eq!(std::fs::read_dir(s.sandbox.join("d")).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&s.sandbox).unwrap().count(), 1);

        assert!(matches!(
            write_file_durable_op(&perms(false, None), &s.sandbox, "d/f", b"x").await,
            Err(FsError::PermissionDenied(_))
        ));
        assert!(matches!(
            write_file_durable_op(&rw, &s.sandbox, "../outside/f", b"x").await,
            Err(FsError::InvalidPath(_))
        ));
    }

    #[test]
    fn test_parse_two_paths() {
        let input = Value::Tuple(vec![Value::String("a".into()), Value::String("b".into())]);
        assert_eq!(parse_two_paths(&input).unwrap(), ("a".into(), "b".into()));
        assert!(parse_two_paths(&Value::String("a".into())).is_err());
    }
}
