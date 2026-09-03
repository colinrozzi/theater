//! The spawn seam — how `theater-core` puts long-lived actor loops on the substrate.
//!
//! The core does not spawn tasks itself; it asks a [`Spawn`] for one. Native
//! provides [`TokioSpawn`] (work-stealing); a browser driver would provide a
//! Worker/`spawn_local`-based one, an embedded driver an embassy task. The core
//! stays agnostic — this is the one capability it needs from its driver to run.
//!
//! A [`TaskHandle`] reproduces the slice of `tokio::task::JoinHandle` the actor
//! lifecycle actually uses: it's a `Future` (await it for completion, poll it in a
//! `select!`) and it can be [`abort`](TaskHandle::abort)ed. Native wraps a real
//! `JoinHandle`; other drivers implement the same contract.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A boxed, `Send` future the core hands to a driver to run.
pub type BoxedTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Reason a task ended abnormally (mirrors what the lifecycle loop inspects).
#[derive(Debug)]
pub struct TaskError(String);

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A handle to a spawned task: `Future` (completion) + [`abort`](Self::abort).
///
/// Native backs this with `tokio::task::JoinHandle`; the actor lifecycle awaits it
/// in `select!`, aborts it, and joins after abort — this exposes exactly that.
pub struct TaskHandle {
    inner: tokio::task::JoinHandle<()>,
}

impl TaskHandle {
    /// Cancel the task. Idempotent; safe after completion.
    pub fn abort(&self) {
        self.inner.abort();
    }

    /// Whether the task has finished (completed, panicked, or was aborted).
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

impl Future for TaskHandle {
    type Output = Result<(), TaskError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // `JoinHandle` is `Unpin`, so this is sound and keeps `&mut TaskHandle`
        // usable directly in `select!`.
        Pin::new(&mut self.inner)
            .poll(cx)
            .map_err(|e| TaskError(e.to_string()))
    }
}

/// The spawn capability a driver gives the core: run a long-lived actor loop.
///
/// The runtime is **generic over its `Spawn`** (`TheaterRuntime<E: Spawn>`), not
/// `dyn`: which substrate a node runs on is a *compile-time* property of the build
/// (a native binary vs a browser wasm binary — never swapped at runtime), so a type
/// parameter is the honest model and makes the runtime's defining axis visible in
/// its type. `Clone` because the runtime hands the executor to each spawned task; a
/// driver's executor is a cheap handle (ZST here; `Rc`/`Arc` elsewhere).
pub trait Spawn: Clone + Send + Sync + 'static {
    fn spawn(&self, task: BoxedTask) -> TaskHandle;
}

/// The native work-stealing driver: `tokio::spawn`. This is `theater-driver-native`
/// in miniature — it will move to its own crate at the split.
#[derive(Clone, Copy, Default)]
pub struct TokioSpawn;

impl Spawn for TokioSpawn {
    fn spawn(&self, task: BoxedTask) -> TaskHandle {
        TaskHandle {
            inner: tokio::spawn(task),
        }
    }
}
