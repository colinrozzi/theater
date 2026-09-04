//! The native driver for the Theater runtime.
//!
//! `theater` (the core) is agnostic about *how* actor loops get put on the
//! substrate — it only asks for a [`theater::executor::Spawn`]. This crate is the
//! native answer: [`TokioSpawn`], a work-stealing driver backed by `tokio::spawn`,
//! plus [`TokioTaskHandle`], its [`theater::executor::SpawnedTask`] handle. A
//! native binary constructs `TheaterRuntime::new(..., TokioSpawn)`; a browser or
//! embedded target would depend on a different driver crate instead.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use theater::executor::{BoxedTask, Spawn, SpawnedTask, TaskError};

/// Native task handle: a thin wrapper over `tokio::task::JoinHandle`.
pub struct TokioTaskHandle {
    inner: tokio::task::JoinHandle<()>,
}

impl SpawnedTask for TokioTaskHandle {
    fn abort(&self) {
        self.inner.abort();
    }

    fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

impl Future for TokioTaskHandle {
    type Output = Result<(), TaskError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // `JoinHandle` is `Unpin`, so this is sound and keeps `&mut TokioTaskHandle`
        // usable directly in `select!`.
        Pin::new(&mut self.inner)
            .poll(cx)
            .map_err(|e| TaskError::new(e.to_string()))
    }
}

/// The native work-stealing driver: `tokio::spawn`.
#[derive(Clone, Copy, Default)]
pub struct TokioSpawn;

impl Spawn for TokioSpawn {
    type Handle = TokioTaskHandle;

    fn spawn(&self, task: BoxedTask) -> Self::Handle {
        TokioTaskHandle {
            inner: tokio::spawn(task),
        }
    }
}
