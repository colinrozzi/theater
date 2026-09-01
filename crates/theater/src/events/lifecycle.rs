//! Fixed, typed vocabulary of an actor's structural lifecycle transitions.
//!
//! These are recorded on an actor's chain (as `ChainEventPayload::Lifecycle`)
//! and are the events that fate-links and monitors key on. Exactly one
//! `Terminated` is emitted per actor, on every death path — it consolidates the
//! ad-hoc `"shutdown"` / `"wasm"`-error `event_type` strings and the
//! relationship-scoped `ActorResult` (`Success`/`Error`/`ExternalStop`) into one
//! place. See `docs/actor-relationships-and-lifecycle.md`.

use crate::pack_bridge::GraphValue;
use serde::{Deserialize, Serialize};

/// A structural transition in an actor's lifecycle.
///
/// `#[derive(GraphValue)]` gives the `Value` case-names from the variant idents
/// (`Terminated`, `Spawned`, …) and tag-encodes them on the wire; the lifecycle
/// `Pattern`s in [`crate::subscription`] key on those idents (`Lifecycle` →
/// `Terminated`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, GraphValue)]
pub enum ActorLifecycleEvent {
    /// Setup + init completed; the actor is now live.
    Spawned,
    /// The actor was paused.
    Paused,
    /// The actor resumed from a paused state.
    Resumed,
    /// The actor terminated. Exactly one, whatever the cause.
    Terminated { cause: TerminationCause },
}

/// Why an actor terminated. This is neutral data — *which* causes a given
/// subscriber reacts to (errors only, any termination, …) is a per-subscription
/// filter decision, made where dispatch happens, not a property of the event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, GraphValue)]
pub enum TerminationCause {
    /// Clean, self-driven exit: init returned, self-shutdown, or a graceful stop
    /// that ran to completion. `final_state` = the actor's last state, if any.
    Completed { final_state: Option<Vec<u8>> },
    /// The guest errored or trapped. `error` is the (structured, at the source)
    /// `ActorError`'s rendering — the chain carries it as a string, like the
    /// existing wasm-error events.
    Failed { error: String },
    /// Stopped by an external graceful command (a supervisor `stop`, the runtime
    /// cascade).
    Stopped,
    /// Brutally force-killed (`TerminateActor`).
    Killed,
    /// Stopped because a fate-linked peer terminated — `peer` is that peer's id.
    /// As the fate cascade ripples, each level records `PeerKilled` naming the
    /// one above it, so the terminal chain reads as a causal chain.
    PeerKilled { peer: String },
}

impl ActorLifecycleEvent {
    /// The `event_type` string recorded on the chain, for filtering/routing.
    /// (The termination *cause* lives in the payload, not this string.)
    pub fn event_type(&self) -> &'static str {
        match self {
            ActorLifecycleEvent::Spawned => "spawned",
            ActorLifecycleEvent::Paused => "paused",
            ActorLifecycleEvent::Resumed => "resumed",
            ActorLifecycleEvent::Terminated { .. } => "terminated",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack_bridge::Value;

    fn roundtrip(e: ActorLifecycleEvent) {
        let v = Value::from(e.clone());
        let back = ActorLifecycleEvent::try_from(v).expect("decode");
        assert_eq!(e, back);
    }

    #[test]
    fn lifecycle_events_roundtrip_through_value() {
        roundtrip(ActorLifecycleEvent::Spawned);
        roundtrip(ActorLifecycleEvent::Paused);
        roundtrip(ActorLifecycleEvent::Resumed);
        roundtrip(ActorLifecycleEvent::Terminated {
            cause: TerminationCause::Completed { final_state: None },
        });
        roundtrip(ActorLifecycleEvent::Terminated {
            cause: TerminationCause::Completed {
                final_state: Some(vec![1, 2, 3]),
            },
        });
        roundtrip(ActorLifecycleEvent::Terminated {
            cause: TerminationCause::Failed {
                error: "boom".into(),
            },
        });
        roundtrip(ActorLifecycleEvent::Terminated {
            cause: TerminationCause::Stopped,
        });
        roundtrip(ActorLifecycleEvent::Terminated {
            cause: TerminationCause::Killed,
        });
        roundtrip(ActorLifecycleEvent::Terminated {
            cause: TerminationCause::PeerKilled {
                peer: "peer-id".into(),
            },
        });
    }
}
