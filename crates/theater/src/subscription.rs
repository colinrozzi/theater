//! The lifecycle-subscription vocabulary shared by the `lifecycle` handler —
//! one directed primitive underlying both **links** (fate-sharing) and
//! **monitors** (watching).
//!
//! Relationships live entirely in the `lifecycle` handler: it subscribes to the
//! subject's chain, matches each event's `Value` against a set of structural
//! [`Pattern`]s (host-side, match-any), and acts per the subscription's
//! [`Target`]. The runtime holds no subscription state of its own. See
//! `docs/actor-relationships-and-lifecycle.md`.

use crate::pack_bridge::Pattern;

/// What happens to the subscriber when one of the subject's events matches —
/// the axis that distinguishes a link from a monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The subscriber is stopped (fate-sharing — a **link**). The `lifecycle`
    /// handler enacts it by issuing `PeerTerminated`; it never enters wasm.
    StopSelf,
    /// The matching event is delivered to the subscriber's wasm via the
    /// `lifecycle` handler's `handle-lifecycle-event` export (a **monitor**).
    DeliverToWasm,
}

/// Pattern matching **any** lifecycle event of the subject (spawned / paused /
/// resumed / terminated), regardless of contents.
///
/// The event `Value` is `Variant("chain-event-payload", "lifecycle", [inner])`,
/// so this pins the outer kind and wildcards the inner event.
pub fn any_lifecycle_event() -> Pattern {
    Pattern::variant("lifecycle", [Pattern::any()])
}

/// Pattern matching any `Terminated` lifecycle event of the subject (any
/// cause) — the fate filter a supervision link keys on. Because a variant's
/// payload arity is pinned by its type, the single termination-cause slot is an
/// explicit [`Pattern::any`].
pub fn any_termination() -> Pattern {
    Pattern::variant(
        "lifecycle",
        [Pattern::variant("terminated", [Pattern::any()])],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::lifecycle::{ActorLifecycleEvent, TerminationCause};
    use crate::events::ChainEventPayload;
    use crate::pack_bridge::{IntoValue, Value};

    fn lifecycle_value(evt: ActorLifecycleEvent) -> Value {
        ChainEventPayload::Lifecycle(evt).into_value()
    }

    #[test]
    fn any_termination_matches_every_cause_but_not_spawn() {
        let pat = any_termination();
        for cause in [
            TerminationCause::Completed { final_state: None },
            TerminationCause::Failed { error: "x".into() },
            TerminationCause::Stopped,
            TerminationCause::Killed,
        ] {
            assert!(pat.matches(&lifecycle_value(ActorLifecycleEvent::Terminated { cause })));
        }
        assert!(!pat.matches(&lifecycle_value(ActorLifecycleEvent::Spawned)));
    }

    #[test]
    fn any_lifecycle_event_matches_spawn_and_terminate() {
        let pat = any_lifecycle_event();
        assert!(pat.matches(&lifecycle_value(ActorLifecycleEvent::Spawned)));
        assert!(
            pat.matches(&lifecycle_value(ActorLifecycleEvent::Terminated {
                cause: TerminationCause::Stopped
            }))
        );
    }
}
