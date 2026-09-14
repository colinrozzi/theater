pub mod formatting;

/// Await the next process shutdown signal, resolving on either SIGINT (Ctrl-C)
/// or SIGTERM, and return its name for logging.
///
/// systemd delivers **SIGTERM** on `systemctl stop`/`restart`; if we only
/// handled Ctrl-C (SIGINT), a service stop would take the OS default action
/// (immediate terminate) and no actor would receive `wait_for_shutdown` — no
/// TLS `close_notify`, in-flight connections cut, listeners freed only by
/// kernel fd reclaim. Handling both drives the same graceful drain.
///
/// SIGKILL is uncatchable by design, so a hard kill remains OS-reclaim-only
/// (best-effort, no drain) — that is expected and cannot be handled here.
#[cfg(unix)]
pub async fn shutdown_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = sigint.recv() => "SIGINT",
        _ = sigterm.recv() => "SIGTERM",
    }
}

/// Non-unix fallback: SIGTERM is a unix concept, so only Ctrl-C is available.
#[cfg(not(unix))]
pub async fn shutdown_signal() -> &'static str {
    let _ = tokio::signal::ctrl_c().await;
    "Ctrl-C"
}
