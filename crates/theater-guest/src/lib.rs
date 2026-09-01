//! Guest-side ergonomics for writing Theater actors.
//!
//! Theater actors are `packr-guest` modules — this crate re-exports it and adds
//! Theater's own domain model on top, so an actor depends on one crate.
//!
//! # In-module state
//!
//! Theater actors own their state **inside the module**. The runtime does not
//! hold, serialize, or thread state through calls: the chain of inputs and
//! outputs is the single source of truth, and state is a private, replayable
//! projection of it (see `docs/in-module-state.md`). An actor keeps its state in
//! a module-global [`StateCell`], sets it once in `init`, and mutates it in place
//! from each export — nothing state-shaped crosses the host boundary.
//!
//! ```ignore
//! use theater_guest::StateCell;
//! use theater_guest::packr_guest::export;
//!
//! struct Counter { count: u64 }
//!
//! static STATE: StateCell<Counter> = StateCell::new();
//!
//! #[export]
//! fn init() {
//!     STATE.set(Counter { count: 0 });   // set once
//! }
//!
//! #[export]
//! fn handle_send(_from: String, _msg: Vec<u8>) {
//!     STATE.with_mut(|c| c.count += 1);  // mutate in place
//! }
//! ```
//!
//! [`StateCell`] is the primitive `#[derive(State)]` builds on: the derive will
//! generate the cell, the accessors, and an optional `get-state` export so the
//! actor stays inspectable. Actors that hold non-serializable state (handles,
//! resources) can use [`StateCell`] directly and simply never expose it.

#![no_std]

/// Re-export of the generic Graph-ABI guest. Actors reach `export`, `import`,
/// `pack_types!`, `Value`, etc. through `theater_guest::packr_guest`.
pub use packr_guest;

/// Derive an actor's managed, inspectable state — a module-global [`StateCell`]
/// plus a `theater:simple/actor.get-state` export. See its docs for the shape.
pub use theater_guest_macros::State;

use core::cell::UnsafeCell;

/// A module-global slot holding an actor's state inside the wasm module.
///
/// Theater actor modules are single-threaded (one instance, no shared-memory
/// threads), so a plain `UnsafeCell` behind a `static` is sound: there is never a
/// second thread to race with, and the borrows handed out by [`with`](Self::with)
/// / [`with_mut`](Self::with_mut) are confined to the closure and never overlap a
/// re-entrant call (an actor export runs to completion before the next dispatch).
///
/// The cell starts empty. [`init`] must [`set`](Self::set) it before any export
/// reads it; reading an unset cell panics (an honest error — the module trapped
/// because it violated its own lifecycle, not because the runtime lost state).
///
/// [`init`]: https://docs.rs/theater-guest
pub struct StateCell<T> {
    slot: UnsafeCell<Option<T>>,
}

// Sound because Theater actor modules are single-threaded; see the type docs.
// The cell is never actually shared across threads at runtime.
unsafe impl<T> Sync for StateCell<T> {}

impl<T> StateCell<T> {
    /// Create an empty cell. `const` so it can back a `static`.
    pub const fn new() -> Self {
        StateCell {
            slot: UnsafeCell::new(None),
        }
    }

    /// Install the actor's state. Call once from `init`; calling again replaces
    /// the current state (a deliberate reset, not the normal path).
    pub fn set(&self, value: T) {
        // Safe: single-threaded, no outstanding borrow (see type docs).
        unsafe {
            *self.slot.get() = Some(value);
        }
    }

    /// Whether the state has been set yet.
    pub fn is_set(&self) -> bool {
        // Safe: single-threaded, shared read only.
        unsafe { (*self.slot.get()).is_some() }
    }

    /// Borrow the state immutably for the duration of `f`.
    ///
    /// Panics if the state was never set (`init` must set it first).
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        // Safe: single-threaded; the borrow lives only inside `f` and an export
        // runs to completion before the next dispatch, so borrows never overlap.
        let state = unsafe {
            (*self.slot.get())
                .as_ref()
                .expect("actor state read before init set it")
        };
        f(state)
    }

    /// Borrow the state mutably for the duration of `f`.
    ///
    /// Panics if the state was never set (`init` must set it first).
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        // Safe: single-threaded; the borrow lives only inside `f` and an export
        // runs to completion before the next dispatch, so borrows never overlap.
        let state = unsafe {
            (*self.slot.get())
                .as_mut()
                .expect("actor state read before init set it")
        };
        f(state)
    }
}

impl<T> Default for StateCell<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_read() {
        let cell: StateCell<u64> = StateCell::new();
        assert!(!cell.is_set());
        cell.set(7);
        assert!(cell.is_set());
        assert_eq!(cell.with(|n| *n), 7);
    }

    #[test]
    fn with_mut_mutates_in_place() {
        let cell: StateCell<u64> = StateCell::new();
        cell.set(0);
        cell.with_mut(|n| *n += 5);
        cell.with_mut(|n| *n += 3);
        assert_eq!(cell.with(|n| *n), 8);
    }

    #[test]
    fn set_replaces() {
        let cell: StateCell<u64> = StateCell::new();
        cell.set(1);
        cell.set(42);
        assert_eq!(cell.with(|n| *n), 42);
    }

    #[test]
    #[should_panic(expected = "actor state read before init set it")]
    fn read_before_set_panics() {
        let cell: StateCell<u64> = StateCell::new();
        cell.with(|n| *n);
    }
}
