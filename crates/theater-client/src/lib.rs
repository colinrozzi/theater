//! # Theater Client
//!
//! This module provides utilities for connecting to and interacting with a Theater server.
//! It offers a simple, flexible interface for sending commands and receiving responses.

mod tcp;

pub use tcp::TheaterConnection;
