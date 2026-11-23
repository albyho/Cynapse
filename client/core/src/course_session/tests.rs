//! Course-session white-box tests. Shared fixtures live in `support.rs`.
//!
//! Submodules:
//! - `course_opening` — open/resume/recovery entry paths
//! - `progress` — course progress and status
//! - `challenge` — Challenge status persistence and reset
//! - `narration` — playback/progress interplay
//! - `recovery` — interrupted-session recovery
//! - `workspace_reset` / `workspace_seed` — workspace lifecycle (unit-scoped seeds)
//! - `seed_top_up` — seed overlay/top-up behavior
//! - `action_paths` — read versus write path policy
//! - `resource_copy` — resource replacement and recovery
//! - `terminal_safety` — terminal command safety gates

mod action_paths;
mod challenge;
mod course_opening;
mod narration;
mod playback;
mod progress;
mod recovery;
mod resource_copy;
mod seed_top_up;
mod support;
mod terminal_safety;
mod workspace_reset;
mod workspace_seed;
