//! Build-time self-contained composition for the packr 0.10.x cutover.
//!
//! `theater build` links the cargo-built actor member with the packr **bundled**
//! allocator (`DEFAULT_ALLOCATOR_WASM`) into a **self-contained composite** — own
//! memory, internalized `pack:alloc`, with the actor's host `theater:simple/*`
//! imports left **residual**. That composite is the artifact theater's 0.10.x
//! loader (`assert_self_contained`) accepts; a bare cargo-built member (imported
//! memory, unresolved `pack:alloc`) is *not* self-contained and will not load.
//!
//! Requires two external tools on PATH (both are in the theater dev shell):
//!   - `wasm-merge` (binaryen) — `packr::link` shells out to it to fuse the composite;
//!   - `wasm-tools`            — the post-compose verification gate.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::process::Command;

use packr::{link, Layout, LinkBinary, DEFAULT_ALLOCATOR_WASM};

/// Link an actor member + the packr bundled allocator into a self-contained
/// composite. Single-package actor: no `[[link]]` edges, so the composite's
/// residual imports are exactly the actor's host interfaces (`theater:simple/*`);
/// `pack:alloc` and the memory are internalized.
///
/// The member must have been built with the fixed-base recipe (see any
/// `test-actors/*/.cargo/config.toml`). Needs `wasm-merge` (binaryen) on PATH.
pub fn compose_self_contained(member: Vec<u8>) -> Result<Vec<u8>> {
    link(
        vec![
            LinkBinary {
                alias: "alloc".into(),
                wasm: DEFAULT_ALLOCATOR_WASM.to_vec(),
                allocator: true,
            },
            LinkBinary {
                alias: "actor".into(),
                wasm: member,
                allocator: false,
            },
        ],
        &[],
        Layout::default(),
    )
    .map_err(|e| {
        anyhow!(
            "failed to link actor member + bundled allocator into a self-contained composite: {e}. \
             Is `wasm-merge` (binaryen) on PATH? Was the member built with the fixed-base recipe \
             (test-actors/*/.cargo/config.toml)?"
        )
    })
}

/// Post-compose gate: assert the composite is genuinely self-contained, so a
/// bad artifact fails the **build** instead of failing at boot.
///
/// The definitive structural check is the import surface: a self-contained
/// actor composite imports **only** host `theater:simple/*` functions — never
/// memory, never the allocator. (`compose_self_contained` succeeding already
/// implies the CGRF `__pack_types` surface survived, since `packr::link`'s
/// `read_surface` would have failed otherwise.)
///
/// Uses `wasm-tools` on PATH.
pub fn verify_self_contained(composite_path: &Path) -> Result<()> {
    // 1) Structural validity.
    let validate = Command::new("wasm-tools")
        .arg("validate")
        .arg(composite_path)
        .output()
        .context("failed to run `wasm-tools validate` — is `wasm-tools` on PATH?")?;
    if !validate.status.success() {
        bail!(
            "composite failed `wasm-tools validate`:\n{}",
            String::from_utf8_lossy(&validate.stderr)
        );
    }

    // 2) Import surface: every residual import must be a host `theater:simple/*`
    //    function. Anything else — an imported memory (`env`/`memory`), the
    //    allocator (`pack:alloc`), `__linear_memory`, etc. — means the allocator
    //    or memory was not internalized (i.e. a bare member was deployed).
    let printed = Command::new("wasm-tools")
        .arg("print")
        .arg(composite_path)
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
            "composite is NOT self-contained: found imports other than host \
             `theater:simple/*` — the allocator/memory was not internalized \
             (did a bare member get built without the fixed-base recipe, or \
             composition get skipped?):\n  {}",
            offenders.join("\n  ")
        );
    }

    Ok(())
}

/// Scan `wasm-tools print` WAT output and return every import declaration whose
/// module is not a host `theater:simple/*` interface. A self-contained actor
/// composite must yield none: an imported memory (`env`/`memory`), the allocator
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
