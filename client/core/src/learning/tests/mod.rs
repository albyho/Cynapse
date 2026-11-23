//! Learning-mode runtime tests. Shared builder `position()` lives below.
//!
//! Submodules:
//! - `modes` — by-step vs continuous mode progression
//! - `narration_and_failure` — narration continuation and failure handling

use super::*;
use crate::{ActionNarrationPhase, NarrationClipContinuation, NarrationClipRef, PlaybackPosition};

fn position() -> PlaybackPosition {
    PlaybackPosition {
        parent_id: "ch".to_string(),
        parent_title: "Grouping".to_string(),
        unit_path: "ch/unit".to_string(),
        unit_title: "Unit".to_string(),
        step_id: "step".to_string(),
        step_title: "Step".to_string(),
        step_started: true,
        step_index: 0,
        step_total: 2,
        unit_step_index: 0,
        unit_step_total: 2,
        action_index: 0,
        action_total: 1,
        overall_action_index: 0,
        overall_action_total: 2,
    }
}

mod modes;
mod narration_and_failure;
