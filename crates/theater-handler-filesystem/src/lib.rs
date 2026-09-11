//! # FileSystem Handler
//!
//! Sandboxed, permission-gated filesystem access for WebAssembly actors in the
//! Theater system. Implements the `theater:simple/filesystem` interface
//! (`filesystem.pact`): read-file / write-file / append-file / delete-file /
//! exists / list-dir / create-dir / remove-dir / metadata.
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
//!   append-file / delete-file / create-dir / remove-dir) require
//!   [`FileSystemPermissions::write`]. A missing capability (or a false bit)
//!   yields `permission-denied`.
//! - **allowed-paths.** When [`FileSystemPermissions::allowed_paths`] is set, the
//!   resolved path must fall within one of those subtrees (interpreted relative
//!   to the sandbox root) or the op is rejected `permission-denied`.
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
/// the listed subtrees (each interpreted relative to the sandbox root).
fn check_allowed(
    root: &Path,
    perms: &FileSystemPermissions,
    candidate: &Path,
    user_path: &str,
) -> Result<(), FsError> {
    if let Some(allowed) = &perms.allowed_paths {
        let ok = allowed.iter().any(|a| {
            resolve_in_sandbox(root, a)
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
    fn resolve_root(&self) -> anyhow::Result<PathBuf> {
        let root = match (&self.path, self.new_dir) {
            (_, Some(true)) => fresh_temp_root(),
            (Some(p), _) => p.clone(),
            (None, _) => fresh_temp_root(),
        };
        std::fs::create_dir_all(&root)
            .with_context(|| format!("creating filesystem sandbox root {:?}", root))?;
        let canonical = std::fs::canonicalize(&root)
            .with_context(|| format!("canonicalizing filesystem sandbox root {:?}", root))?;
        Ok(canonical)
    }
}

/// A fresh, process-and-time-unique temp directory path (not yet created).
fn fresh_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("theater-fs-{}-{}", std::process::id(), nanos))
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
        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("FileSystem handler received shutdown signal");
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
}
