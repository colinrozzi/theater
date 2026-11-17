//! What an actor uses, kept as it runs and read without calling into it: the
//! size of its linear memory (as of its latest call), and how many calls the
//! runtime has made into it (`ActorOperation::CallFunctionPack`: init,
//! messages, handler callbacks) and the wall time they took, host calls the
//! guest awaited included. Read-only: nothing here limits an actor.
//!
//! The actor's runtime writes the counters after each call; the runtime's
//! command loop reads them for `TheaterCommand::GetActorUsage` from its own
//! map, so a busy actor still answers, with the numbers as of its last call.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The live counters, shared by an actor's instance and the runtime.
#[derive(Debug, Default)]
pub struct UsageCounters {
    memory_bytes: AtomicU64,
    calls: AtomicU64,
    call_nanos: AtomicU64,
}

impl UsageCounters {
    /// One call into the actor took `took`; its memory is now `memory_bytes`.
    pub fn record_call(&self, took: Duration, memory_bytes: usize) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.call_nanos.fetch_add(
            u64::try_from(took.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        self.set_memory(memory_bytes);
    }

    pub fn set_memory(&self, memory_bytes: usize) {
        self.memory_bytes
            .store(memory_bytes as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ActorUsage {
        ActorUsage {
            memory_bytes: self.memory_bytes.load(Ordering::Relaxed),
            calls: self.calls.load(Ordering::Relaxed),
            call_time: Duration::from_nanos(self.call_nanos.load(Ordering::Relaxed)),
        }
    }
}

/// A reading of [`UsageCounters`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorUsage {
    /// Linear memory, in bytes, after the latest call (memory only grows).
    pub memory_bytes: u64,
    /// Calls made into the actor.
    pub calls: u64,
    /// Their total wall time.
    pub call_time: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_add_up_and_memory_is_the_latest() {
        let usage = UsageCounters::default();
        usage.set_memory(65536);
        usage.record_call(Duration::from_millis(3), 131072);
        usage.record_call(Duration::from_millis(4), 131072);
        assert_eq!(
            usage.snapshot(),
            ActorUsage {
                memory_bytes: 131072,
                calls: 2,
                call_time: Duration::from_millis(7)
            }
        );
    }
}
