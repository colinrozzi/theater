//! The spawn seam — how `theater` puts long-lived actor loops on the substrate.
//!
//! The core does not spawn tasks itself; it asks a [`Spawn`] for one and holds the
//! result as an opaque [`SpawnedTask`]. The native work-stealing driver
//! (`theater-native`'s `TokioSpawn`) provides both; a browser driver would provide
//! a Worker/`spawn_local`-based one, an embedded driver an embassy task. The core
//! stays agnostic — [`Spawn`] is the one capability it needs from its driver to run.
//!
//! [`SpawnedTask`] reproduces the slice of `tokio::task::JoinHandle` the actor
//! lifecycle actually uses: it's a `Future` (await it for completion, poll it in a
//! `select!`), it can be [`abort`](SpawnedTask::abort)ed, and its liveness can be
//! polled with [`is_finished`](SpawnedTask::is_finished). A driver names its own
//! concrete handle via [`Spawn::Handle`].

use std::future::Future;
use std::pin::Pin;

/// A boxed, `Send` future the core hands to a driver to run.
pub type BoxedTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Reason a task ended abnormally (mirrors what the lifecycle loop inspects).
#[derive(Debug)]
pub struct TaskError(String);

impl TaskError {
    /// Build a `TaskError` from a driver's join failure (panic / cancellation).
    /// Drivers construct this when reporting an abnormal task end to the core.
    pub fn new(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }
}

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A handle to a spawned actor loop, as the core consumes it.
///
/// The actor lifecycle awaits it in a `select!` (hence `Future` + `Unpin`, so
/// `&mut handle` is itself a future), [`abort`](Self::abort)s it, joins after
/// abort, and checks [`is_finished`](Self::is_finished). A driver's concrete handle
/// (native: a `tokio::task::JoinHandle` wrapper) implements this contract.
pub trait SpawnedTask:
    Future<Output = Result<(), TaskError>> + Unpin + Send + Sync + 'static
{
    /// Cancel the task. Idempotent; safe after completion.
    fn abort(&self);

    /// Whether the task has finished (completed, panicked, or was aborted).
    fn is_finished(&self) -> bool;
}

/// The spawn capability a driver gives the core: run a long-lived actor loop.
///
/// The runtime is **generic over its `Spawn`** (`TheaterRuntime<E: Spawn>`), not
/// `dyn`: which substrate a node runs on is a *compile-time* property of the build
/// (a native binary vs a browser wasm binary — never swapped at runtime), so a type
/// parameter is the honest model and makes the runtime's defining axis visible in
/// its type. The spawned handle is a driver-chosen associated type ([`Self::Handle`])
/// so the core never names a concrete (native) handle — that's what lets the driver
/// live in its own crate. `Clone` because the runtime hands the executor to each
/// spawned task; a driver's executor is a cheap handle (ZST for native; `Rc`/`Arc`
/// elsewhere).
pub trait Spawn: Clone + Send + Sync + 'static {
    /// The concrete task handle this driver produces.
    type Handle: SpawnedTask;

    fn spawn(&self, task: BoxedTask) -> Self::Handle;
}
