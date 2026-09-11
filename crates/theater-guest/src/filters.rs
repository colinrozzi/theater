//! Monitor filter presets.
//!
//! Convenience constructors for the common `monitor-filtered` filters, built on
//! top of the raw structural matcher [`Pattern`]. A bare `monitor(subject)`
//! watches the subject's **whole chain**; `monitor-filtered(subject, filter)`
//! narrows that stream to just the events the watcher cares about. `filter` is a
//! serialized `packr_abi::Pattern` — a [`Value`] the host decodes with its
//! `TryFrom<Value>` — so these helpers return a ready-to-pass [`Value`].
//!
//! ```ignore
//! use theater_guest::filters;
//! // Only wake me when the subject terminates:
//! monitor_filtered(subject, filters::terminations());
//! ```
//!
//! The shapes mirror the host's `any_termination()` / `any_lifecycle_event()`:
//! a chain event's `Value` is `Variant("Lifecycle", [inner])` (from the
//! `ChainEventPayload` derive), so a lifecycle filter pins the outer `Lifecycle`
//! kind and a terminations filter additionally pins the inner `Terminated` case.

use packr_guest::composite_abi::Pattern;
use packr_guest::Value;

/// A filter matching any `Terminated` lifecycle event of the subject (any
/// cause) — the "only wake me on death" preset. Mirrors the host's
/// `any_termination()`.
pub fn terminations() -> Value {
    Value::from(Pattern::variant(
        "Lifecycle",
        [Pattern::variant("Terminated", [Pattern::any()])],
    ))
}

/// A filter matching any lifecycle event of the subject (spawned / paused /
/// resumed / terminated), regardless of contents. Mirrors the host's
/// `any_lifecycle_event()`.
pub fn lifecycle() -> Value {
    Value::from(Pattern::variant("Lifecycle", [Pattern::any()]))
}
