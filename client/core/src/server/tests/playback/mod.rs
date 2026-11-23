//! Playback scenario tests, grouped by concern. Shared helpers
//! (`wait_for_narrating`, `one_step_server`, clip builders) live below.
//!
//! Submodules:
//! - `basics` — basic playback lifecycle
//! - `resume` — resume positions and interrupted playback
//! - `boundaries` — step/unit boundary timing
//! - `tail` — tail completion behavior
//! - `narration` — narration phase gating (see narration/mod.rs)
//! - `interruption` — mid-playback interruption
//! - `failure` — media/adapter failure handling
//! - `integration` — cross-cutting integration cases

use super::super::*;
use super::support::*;
use crate::course::{
    ActionNarration, ActionNarrationClip, ActionNarrationPhaseClips, CreateDirectoryAction,
    CreateFileAction, DeleteFileAction, MissingTargetPolicy, PlaybackAction, PlaybackStep,
    TerminalCommandAction,
};
use std::path::Path;

mod basics;
mod boundaries;
mod failure;
mod integration;
mod interruption;
mod narration;
mod resume;
mod tail;

async fn wait_for_narrating(
    playback: &mut tokio::sync::watch::Receiver<PlaybackState>,
    phase: ActionNarrationPhase,
    action_index: usize,
    clip_index: usize,
) -> NarrationClipRef {
    loop {
        if let PlaybackState::Narrating { clip, .. } = &*playback.borrow()
            && clip.phase == phase
            && clip.action_index == action_index
            && clip.clip_index == clip_index
        {
            return clip.clone();
        }
        playback.changed().await.unwrap();
    }
}

fn action_narration_clip() -> ActionNarrationClip {
    ActionNarrationClip {
        audio_path: Some(std::path::PathBuf::from("before.ogg")),
        audio_relative_path: Some(std::path::PathBuf::from("before.ogg")),
        offset_ms: Some(100),
        duration_ms: 700,
        caption: Some("Before narration".to_string()),
    }
}

fn action_narration_clip_for_phase(caption: &str) -> ActionNarrationClip {
    ActionNarrationClip {
        audio_path: Some(std::path::PathBuf::from(format!("{caption}.ogg"))),
        audio_relative_path: Some(std::path::PathBuf::from(format!("{caption}.ogg"))),
        offset_ms: Some(100),
        duration_ms: 3000,
        caption: Some(caption.to_string()),
    }
}

async fn one_step_server(
    directory: &Path,
    actions: Vec<PlaybackAction>,
) -> (ServerHandle, ClientSocket) {
    let workspace = directory.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let handle = start_server_with_path(
        workspace,
        directory.join("runtime.json"),
        vec![PlaybackStep {
            parent_id: "ch".to_string(),
            parent_title: "Grouping".to_string(),
            unit_path: "unit".to_string(),
            unit_title: "Unit".to_string(),
            step_id: "step".to_string(),
            step_title: "Step".to_string(),
            requires_human: false,
            challenge: false,
            actions,
        }],
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;
    (handle, socket)
}
