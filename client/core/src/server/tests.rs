//! Server white-box tests. Shared fixtures live in `support.rs`.
//!
//! Submodules:
//! - `handshake` — hello/token/connection setup
//! - `commands` — playback command handling
//! - `playback` — playback timing/boundary scenarios (see playback/mod.rs)
//! - `session` — PlaybackSession progression/failure decisions

mod commands;
mod handshake;
mod playback;
mod session;
mod support;
