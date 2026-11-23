//! Scenario Runner: full-stack protocol tests over a real WebSocket
//! connection (black-box; only `cynapse_client_core` public API).
//!
//! Submodules (wired via #[path], see docs testing layout notes):
//! - test-owned course package builders
//! - `actions` — action apply/terminal-failure flows
//! - `scripting` — scripted disconnect/cancel/terminal behavior
//! - `narration` — narration event flows
//! - `cli` — CLI argument handling
//! - `modes` — by-step/presentation/continuous modes
//! - `continuous` — continuous traversal scenarios

use cynapse_client_core::{AckPayload, AckStatus, ChallengeStatus, CourseSession, MessageType};
use cynapse_client_harness::{
    AdapterDirection, AdapterReply, AdapterScript, NarrationReply, ScenarioError,
    ScenarioEventKind, ScenarioMode, ScenarioOptions, ScenarioSpec, ScenarioStatus, run_scenario,
    run_scenario_with_options, run_scenario_with_setup,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

mod common;

use common::{fixture, unit_by_path};

#[path = "scenario_runner/actions.rs"]
mod actions;
#[path = "scenario_runner/challenge.rs"]
mod challenge;
#[path = "scenario_runner/cli.rs"]
mod cli;
#[path = "scenario_runner/continuous.rs"]
mod continuous;
#[path = "scenario_runner/fixtures.rs"]
mod fixtures;
#[path = "scenario_runner/modes.rs"]
mod modes;
#[path = "scenario_runner/narration.rs"]
mod narration;
#[path = "scenario_runner/scripting.rs"]
mod scripting;
