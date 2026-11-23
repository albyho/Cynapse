//! Narration gating tests, split by narration phase.
//!
//! Submodules:
//! - `before` — before-phase ordering vs action dispatch
//! - `during` — during-phase serialization and tail gating
//! - `wait` — Wait timer, audio, replay and failure semantics
//! - `step_completion` — step completion gates (incl. human steps)
//! - `after` — after-phase ordering vs action ack

use super::*;

mod after;
mod before;
mod during;
mod step_completion;
mod wait;
