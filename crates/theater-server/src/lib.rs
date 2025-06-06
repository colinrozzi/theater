//! # Theater Server
//!
//! HTTP server for Theater actor system management.

mod fragmenting_codec;
mod server;

pub use server::{ManagementCommand, ManagementError, ManagementResponse, TheaterServer};
