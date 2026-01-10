//! Replay-related events for tracking replay completion and verification.

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
