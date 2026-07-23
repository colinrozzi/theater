//! Self-contained **verification** for the packr 0.11.0 plain-build model.
//!
//! As of packr 0.11.0 there is no composition step: an actor is a PLAIN cargo
//! build. `packr_guest::setup_guest!()` links the allocator (dlmalloc) into the
//! cdylib, so the built `.wasm` already exports its own (growable) memory +
//! `__pack_alloc`/`__pack_free` + lifecycle and imports only host
//! `theater:simple/*` interfaces. That bare `.wasm` is what theater's loader
//! accepts directly — no `packr::link`, no bundled allocator, no fixed-base
//! recipe. (Historically this module linked member + `DEFAULT_ALLOCATOR_WASM`
//! into a composite; all of that machinery was removed in 0.11.0.)
//!
//! What remains is the post-build **gate**: assert the built artifact is
//! genuinely self-contained (imports only host functions), so a bad build fails
//! the build instead of failing at boot. Uses `wasm-tools` on PATH.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Post-build gate: assert a plain-built actor `.wasm` is genuinely
/// self-contained, so a bad artifact fails the **build** instead of at boot.
///
/// The definitive structural check is the import surface: a self-contained
/// actor imports **only** host `theater:simple/*` functions — never memory,
/// never an allocator (`pack:alloc`). On the 0.11.0 plain-build model the
/// allocator + memory are internal to the cdylib, so a correct build yields no
/// offending imports; an imported memory or `pack:alloc` means the actor was
/// built wrong (e.g. with the retired fixed-base `--import-memory` recipe).
///
/// Uses `wasm-tools` on PATH.
pub fn verify_self_contained(actor_path: &Path) -> Result<()> {
    // 1) Structural validity.
    let validate = Command::new("wasm-tools")
        .arg("validate")
        .arg(actor_path)
        .output()
        .context("failed to run `wasm-tools validate` — is `wasm-tools` on PATH?")?;
    if !validate.status.success() {
        bail!(
            "actor wasm failed `wasm-tools validate`:\n{}",
            String::from_utf8_lossy(&validate.stderr)
        );
    }

    // 2) Import surface: every import must be a host `theater:simple/*`
    //    function. Anything else — an imported memory (`env`/`memory`), the
    //    allocator (`pack:alloc`), `__linear_memory`, etc. — means the actor is
    //    not self-contained (a bare member built with the retired --import-memory
    //    recipe instead of the plain 0.11.0 build).
    let printed = Command::new("wasm-tools")
        .arg("print")
        .arg(actor_path)
        .output()
        .context("failed to run `wasm-tools print` — is `wasm-tools` on PATH?")?;
    if !printed.status.success() {
        bail!(
            "`wasm-tools print` failed:\n{}",
            String::from_utf8_lossy(&printed.stderr)
        );
    }
    let wat = String::from_utf8_lossy(&printed.stdout);
    let offenders = non_host_imports(&wat);

    if !offenders.is_empty() {
        bail!(
            "actor is NOT self-contained: found imports other than host \
             `theater:simple/*` — memory or the allocator was not internalized \
             (was it built plain with packr-guest 0.11.0, or did an old \
             --import-memory member slip in?):\n  {}",
            offenders.join("\n  ")
        );
    }

    Ok(())
}

/// Scan `wasm-tools print` WAT output and return every import declaration whose
/// module is not a host `theater:simple/*` interface. A self-contained actor
/// must yield none: an imported memory (`env`/`memory`), the allocator
/// (`pack:alloc`), `__linear_memory`, etc. all carry non-`theater:simple/`
/// modules and so surface here.
fn non_host_imports(wat: &str) -> Vec<String> {
    let mut offenders = Vec::new();
    for line in wat.lines() {
        let l = line.trim_start();
        // Import declarations print as `(import "<module>" "<name>" ...)`.
        if let Some(rest) = l.strip_prefix("(import \"") {
            let module = rest.split('"').next().unwrap_or("");
            if !module.starts_with("theater:simple/") {
                offenders.push(l.trim_end().to_string());
            }
        }
    }
    offenders
}

#[cfg(test)]
mod tests {
    use super::non_host_imports;

    #[test]
    fn accepts_only_host_imports() {
        let wat = r#"
        (module
          (import "theater:simple/runtime" "log" (func (param i32 i32)))
          (import "theater:simple/message-server-host" "register" (func (result i32)))
          (func $f)
        )"#;
        assert!(non_host_imports(wat).is_empty());
    }

    #[test]
    fn flags_imported_memory() {
        let wat = r#"
        (module
          (import "env" "memory" (memory 1))
          (import "theater:simple/runtime" "log" (func (param i32 i32)))
        )"#;
        let bad = non_host_imports(wat);
        assert_eq!(bad.len(), 1);
        assert!(bad[0].contains("\"env\""), "got: {:?}", bad);
    }

    #[test]
    fn flags_imported_allocator() {
        let wat = r#"(module
          (import "pack:alloc" "alloc" (func (param i32) (result i32)))
        )"#;
        let bad = non_host_imports(wat);
        assert_eq!(bad.len(), 1);
        assert!(bad[0].contains("pack:alloc"), "got: {:?}", bad);
    }

    #[test]
    fn ignores_non_import_lines_mentioning_import() {
        // A comment or export that merely contains the word must not trip it.
        let wat = r#"(module
          (; import is a great feature ;)
          (export "handle-send" (func 0))
        )"#;
        assert!(non_host_imports(wat).is_empty());
    }
}
