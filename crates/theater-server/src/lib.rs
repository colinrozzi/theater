//! # Theater Server
//!
//! HTTP server for Theater actor system management.

mod server;

pub use server::{ManagementCommand, ManagementError, ManagementResponse, TheaterServer};
