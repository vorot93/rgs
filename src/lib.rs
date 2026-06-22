//! Asynchronous utilities for querying game servers.
//!
//! The `rgs` crate provides tools to asynchronously retrieve game server
//! information like IP, server metadata, player list and more.

pub mod dns;
pub mod model;
pub mod ping;
pub mod protocol;
