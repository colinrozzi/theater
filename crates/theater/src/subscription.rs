//! The lifecycle-subscription substrate — one directed primitive underlying
//! both **links** (fate-sharing) and **monitors** (watching).
//!
//! A subscription is stored on the **subject** (the actor being watched), in
//! its [`ActorProcess.subscribers`](crate::theater_runtime) map keyed by
//! subscriber. On each of the subject's chain events, the runtime matches the
//! event's `Value` against each subscription's `filter` (host-side, match-any)
//! and, on a match, acts per its [`Target`]. See
//! `docs/actor-relationships-and-lifecycle.md`.

use crate::id::TheaterId;
use crate::pack_bridge::{Pattern, Value};

/// What the runtime does to the subscriber when one of the subject's events
/// matches — the axis that distinguishes a link from a monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The subscriber is stopped by the runtime — fate-sharing (a **link**).
    /// Enacted in the death funnel; the notification never enters wasm.
    StopSelf,
    /// The matching event is delivered to the subscriber's wasm via the
    /// `lifecycle` handler's `handle-lifecycle-event` export (a **monitor**).
    DeliverToWasm,
}

/// A directed subscription: "`subscriber` cares about the subject this is
/// stored on."
///
/// `filter` is a **match-any** set of structural [`Pattern`]s over the subject's
/// event `Value` (the [`ChainEventPayload::into_value`](crate::events) form).
/// An empty filter matches nothing (an inert subscription).
#[derive(Debug, Clone)]
pub struct Subscription {
    /// Who attached. For [`Target::StopSelf`] this is who gets stopped; for
    /// [`Target::DeliverToWasm`] this is who receives. Actor-created
    /// subscriptions always set this to the caller (self-service).
    pub subscriber: TheaterId,
    /// Match-any set of patterns over the event `Value`.
    pub filter: Vec<Pattern>,
    /// What the runtime does on a match.
    pub target: Target,
}

impl Subscription {
    /// Does `event_value` match this subscription's filter? (Match-any across
    /// the pattern set; an empty filter never matches.)
    pub fn matches(&self, event_value: &Value) -> bool {
        self.filter.iter().any(|p| p.matches(event_value))
    }
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
    use crate::pack_bridge::IntoValue;

    fn lifecycle_value(evt: ActorLifecycleEvent) -> Value {
        ChainEventPayload::Lifecycle(evt).into_value()
    }

    #[test]
    fn any_termination_matches_every_cause_but_not_spawn() {
        let sub = Subscription {
            subscriber: TheaterId::generate(),
            filter: vec![any_termination()],
            target: Target::StopSelf,
        };
        for cause in [
            TerminationCause::Completed { final_state: None },
            TerminationCause::Failed { error: "x".into() },
            TerminationCause::Stopped,
            TerminationCause::Killed,
        ] {
            assert!(sub.matches(&lifecycle_value(ActorLifecycleEvent::Terminated { cause })));
        }
        assert!(!sub.matches(&lifecycle_value(ActorLifecycleEvent::Spawned)));
    }

    #[test]
    fn any_lifecycle_event_matches_spawn_and_terminate() {
        let sub = Subscription {
            subscriber: TheaterId::generate(),
            filter: vec![any_lifecycle_event()],
            target: Target::DeliverToWasm,
        };
        assert!(sub.matches(&lifecycle_value(ActorLifecycleEvent::Spawned)));
        assert!(
            sub.matches(&lifecycle_value(ActorLifecycleEvent::Terminated {
                cause: TerminationCause::Stopped
            }))
        );
    }

    #[test]
    fn empty_filter_is_inert() {
        let sub = Subscription {
            subscriber: TheaterId::generate(),
            filter: vec![],
            target: Target::DeliverToWasm,
        };
        assert!(!sub.matches(&lifecycle_value(ActorLifecycleEvent::Spawned)));
    }
}
