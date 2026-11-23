//! Client-state reducer tests. Shared builders
//! (`course_state`, `playback_position`) live below.
//!
//! Submodules:
//! - `transitions` — transition state and retryability
//! - `warnings` — warning identity and render-time text

use super::*;
use crate::{
    ClientTransitionRequirement, ClientTransitionRuntime, ClientTransitionTicket,
    CourseSessionState, CourseTreeNodeView, CourseView, PlaybackPosition, PlaybackState,
    StopTransitionRequest, TransitionRequest, UnitStatus, UnitView,
};

fn course_state() -> CourseSessionState {
    CourseSessionState {
        active_course: Some(CourseView {
            id: "course".to_string(),
            title: "Course".to_string(),
            version: "1.0.0".to_string(),
            adapters: Vec::new(),
            workspace_path: None,
            tree: vec![CourseTreeNodeView {
                path: "chapter/unit".to_string(),
                title: "Unit".to_string(),
                completed: false,
                unit: Some(UnitView {
                    path: "chapter/unit".to_string(),
                    title: "Unit".to_string(),
                    steps: Vec::new(),
                    current_step_id: None,
                    adapters: Vec::new(),
                    status: UnitStatus::InProgress,
                    completed: false,
                    skipped_challenges: 0,
                }),
                children: Vec::new(),
            }],
            current_unit_path: None,
            current_step_id: None,
            recovery: None,
        }),
        active_course_path: Some("course".to_string()),
        selected_unit: None,
        recent_courses: Vec::new(),
        changed_unit_paths: Vec::new(),
        progress_revision: 0,
    }
}

fn playback_position() -> PlaybackPosition {
    PlaybackPosition {
        parent_id: "chapter".to_string(),
        parent_title: "Chapter".to_string(),
        unit_path: "chapter/unit".to_string(),
        unit_title: "Unit".to_string(),
        step_id: "step".to_string(),
        step_title: "Step".to_string(),
        step_started: true,
        step_index: 0,
        step_total: 1,
        unit_step_index: 0,
        unit_step_total: 1,
        action_index: 0,
        action_total: 1,
        overall_action_index: 0,
        overall_action_total: 1,
    }
}

mod transitions;
mod warnings;
