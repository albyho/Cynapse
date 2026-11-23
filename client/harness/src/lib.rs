//! Scenario harness: drives a full course scenario against a real server
//! and adapter over WebSocket, scripting inputs and capturing artifacts.
//!
//! This crate is tooling, not Client runtime: it exists for
//! `cynapse-scenario`, `cynapse-content-verify` and the scenario tests, and
//! depends on `cynapse-client-core` only through its public API.
//!
//! Public API re-exported at this root (`pub use types::*`,
//! `pub use runner::{run_scenario, run_scenario_in_place_with_options,
//! run_scenario_with_setup}`).
//!
//! Submodules:
//! - `types` — public spec/report/error types, `ClientSocket` alias
//! - `runner` — `run_scenario` family and the adapter execution loop
//! - `fake_fs` — fake file system and edit application/recovery helpers
//! - `artifacts` — report writing and reproduction command builders

use cynapse_client_core::course_session::CourseSession;
use cynapse_client_core::discovery::DiscoveryFile;
use cynapse_client_core::protocol::{
    AckPayload, AckStatus, CancelReason, CheckPayload, Envelope, MessageType,
};
use cynapse_client_core::server::{
    AdapterTarget, PlaybackInterruptionReason, PlaybackState, ServerError, ServerHandle,
    start_server_with_path_and_resume_for_adapter,
};
use cynapse_client_core::{AdapterId, LearningController, PlaybackPosition};
use cynapse_course::Action;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use uuid::Uuid;

mod artifacts;
mod fake_fs;
mod runner;
mod types;

pub use runner::{
    run_scenario, run_scenario_in_place_with_options, run_scenario_with_options,
    run_scenario_with_setup,
};
pub use types::*;
