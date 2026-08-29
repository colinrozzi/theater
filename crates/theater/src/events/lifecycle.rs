//! Fixed, typed vocabulary of an actor's structural lifecycle transitions.
//!
//! These are recorded on an actor's chain (as `ChainEventPayload::Lifecycle`)
//! and are the events that fate-links and monitors key on. Exactly one
//! `Terminated` is emitted per actor, on every death path — it consolidates the
//! ad-hoc `"shutdown"` / `"wasm"`-error `event_type` strings and the
//! relationship-scoped `ActorResult` (`Success`/`Error`/`ExternalStop`) into one
//! place. See `docs/actor-relationships-and-lifecycle.md`.

use crate::pack_bridge::{ConversionError, FromValue, IntoValue, Value};
use serde::{Deserialize, Serialize};

/// A structural transition in an actor's lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

impl IntoValue for TerminationCause {
    fn into_value(self) -> Value {
        let (tag, case, payload) = match self {
            TerminationCause::Completed { final_state } => {
                (0, "completed", vec![final_state.into_value()])
            }
            TerminationCause::Failed { error } => (1, "failed", vec![Value::String(error)]),
            TerminationCause::Stopped => (2, "stopped", vec![]),
            TerminationCause::Killed => (3, "killed", vec![]),
        };
        Value::Variant {
            type_name: "termination-cause".into(),
            case_name: case.into(),
            tag,
            payload,
        }
    }
}

impl TryFrom<Value> for TerminationCause {
    type Error = ConversionError;
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Variant {
                case_name, payload, ..
            } => match case_name.as_str() {
                "completed" => {
                    let v = payload
                        .into_iter()
                        .next()
                        .ok_or_else(|| ConversionError::MissingField("final_state".into()))?;
                    Ok(TerminationCause::Completed {
                        final_state: Option::<Vec<u8>>::from_value(v)?,
                    })
                }
                "failed" => {
                    let error = match payload.into_iter().next() {
                        Some(v) => String::try_from(v)?,
                        None => String::new(),
                    };
                    Ok(TerminationCause::Failed { error })
                }
                "stopped" => Ok(TerminationCause::Stopped),
                "killed" => Ok(TerminationCause::Killed),
                other => Err(ConversionError::ExpectedVariant(format!(
                    "unknown case: {}",
                    other
                ))),
            },
            other => Err(ConversionError::ExpectedVariant(format!("{:?}", other))),
        }
    }
}

impl IntoValue for ActorLifecycleEvent {
    fn into_value(self) -> Value {
        let (tag, case, payload) = match self {
            ActorLifecycleEvent::Spawned => (0, "spawned", vec![]),
            ActorLifecycleEvent::Paused => (1, "paused", vec![]),
            ActorLifecycleEvent::Resumed => (2, "resumed", vec![]),
            ActorLifecycleEvent::Terminated { cause } => {
                (3, "terminated", vec![cause.into_value()])
            }
        };
        Value::Variant {
            type_name: "actor-lifecycle-event".into(),
            case_name: case.into(),
            tag,
            payload,
        }
    }
}

impl TryFrom<Value> for ActorLifecycleEvent {
    type Error = ConversionError;
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Variant {
                case_name, payload, ..
            } => match case_name.as_str() {
                "spawned" => Ok(ActorLifecycleEvent::Spawned),
                "paused" => Ok(ActorLifecycleEvent::Paused),
                "resumed" => Ok(ActorLifecycleEvent::Resumed),
                "terminated" => {
                    let cause = payload
                        .into_iter()
                        .next()
                        .ok_or_else(|| ConversionError::MissingField("cause".into()))?;
                    Ok(ActorLifecycleEvent::Terminated {
                        cause: TerminationCause::try_from(cause)?,
                    })
                }
                other => Err(ConversionError::ExpectedVariant(format!(
                    "unknown case: {}",
                    other
                ))),
            },
            other => Err(ConversionError::ExpectedVariant(format!("{:?}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(e: ActorLifecycleEvent) {
        let v = e.clone().into_value();
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
    }
}
