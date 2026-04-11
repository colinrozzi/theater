//! Replay-related events for tracking replay completion and verification.

use crate::pack_bridge::{ConversionError, IntoValue, Value};
use serde::{Deserialize, Serialize};

/// Summary of a replay execution.
///
/// This event is recorded at the end of a replay to indicate completion
/// and provide verification status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySummary {
    /// Total number of events in the original chain
    pub total_events: usize,
    /// Number of events successfully replayed
    pub events_replayed: usize,
    /// Number of hash mismatches detected
    pub mismatches: usize,
    /// Whether the replay completed successfully (all events matched)
    pub success: bool,
    /// Optional error message if replay failed
    pub error: Option<String>,
}

impl ReplaySummary {
    /// Create a successful replay summary
    pub fn success(total_events: usize, events_replayed: usize) -> Self {
        Self {
            total_events,
            events_replayed,
            mismatches: 0,
            success: true,
            error: None,
        }
    }

    /// Create a replay summary with mismatches
    pub fn with_mismatches(total_events: usize, events_replayed: usize, mismatches: usize) -> Self {
        Self {
            total_events,
            events_replayed,
            mismatches,
            success: mismatches == 0,
            error: if mismatches > 0 {
                Some(format!("{} hash mismatches detected", mismatches))
            } else {
                None
            },
        }
    }

    /// Create a failed replay summary with an error
    pub fn failure(total_events: usize, events_replayed: usize, error: String) -> Self {
        Self {
            total_events,
            events_replayed,
            mismatches: 0,
            success: false,
            error: Some(error),
        }
    }
}

impl IntoValue for ReplaySummary {
    fn into_value(self) -> Value {
        Value::Record {
            type_name: String::from("replay-summary"),
            fields: vec![
                ("total-events".into(), Value::U64(self.total_events as u64)),
                ("events-replayed".into(), Value::U64(self.events_replayed as u64)),
                ("mismatches".into(), Value::U64(self.mismatches as u64)),
                ("success".into(), Value::Bool(self.success)),
                ("error".into(), self.error.into_value()),
            ],
        }
    }
}

impl TryFrom<Value> for ReplaySummary {
    type Error = ConversionError;

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Record { fields, .. } => {
                let mut total_events = 0u64;
                let mut events_replayed = 0u64;
                let mut mismatches = 0u64;
                let mut success = false;
                let mut error = None;

                for (name, val) in fields {
                    match name.as_str() {
                        "total-events" => total_events = u64::try_from(val)?,
                        "events-replayed" => events_replayed = u64::try_from(val)?,
                        "mismatches" => mismatches = u64::try_from(val)?,
                        "success" => success = bool::try_from(val)?,
                        "error" => {
                            if let Value::Option { value: Some(v), .. } = val {
                                error = Some(String::try_from(*v)?);
                            }
                        }
                        _ => {}
                    }
                }

                Ok(ReplaySummary {
                    total_events: total_events as usize,
                    events_replayed: events_replayed as usize,
                    mismatches: mismatches as usize,
                    success,
                    error,
                })
            }
            other => Err(ConversionError::ExpectedRecord(format!("{:?}", other))),
        }
    }
}
