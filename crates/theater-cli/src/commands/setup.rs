//! `theater setup` — set up an actor but do NOT call `actor.init`.
//!
//! See [`crate::commands::spawn`] for the shared implementation. `setup`
//! exists as a sibling subcommand for symmetry with `spawn`: both work
//! against the same manifest, but `spawn` is "ready to receive work" and
//! `setup` is "the task loops are up; you drive init yourself".

pub use crate::commands::spawn::{execute_setup as execute_async, SetupArgs};
