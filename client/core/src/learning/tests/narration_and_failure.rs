use super::*;

#[test]
fn action_narration_continuation_respects_the_selected_learning_mode() {
    let completed = PlaybackState::StepComplete {
        position: position(),
        continuation: Some(NarrationClipContinuation {
            clip: NarrationClipRef {
                step_run_id: 1,
                action_index: 0,
                phase: ActionNarrationPhase::During,
                clip_index: 0,
            },
        }),
        requires_human: false,
    };
    let mut by_step = LearningController::default();
    by_step.start(LearningMode::ByStep, "ch/unit").unwrap();
    let waiting = by_step.observe(&completed);
    assert_eq!(waiting.phase, LearningPhase::WaitingForStep);

    let mut continuous = LearningController::default();
    continuous
        .start(LearningMode::Continuous, "ch/unit")
        .unwrap();
    let running = continuous.observe(&completed);
    assert_eq!(running.phase, LearningPhase::Running);
}

#[test]
fn completing_the_final_unit_finishes_continuous_learning() {
    let mut controller = LearningController::default();
    controller
        .start(LearningMode::Continuous, "ch/unit")
        .unwrap();

    let completed = controller.complete_unit();

    assert_eq!(completed.phase, LearningPhase::Completed);
    assert_eq!(completed.mode, None);
    assert_eq!(completed.unit_path, None);
}

#[test]
fn failure_exposes_an_actionable_message_without_losing_the_active_mode() {
    let mut controller = LearningController::default();
    controller
        .start(LearningMode::Continuous, "ch/unit")
        .unwrap();

    let failed = controller.fail("continuous transition environment failed");

    assert_eq!(failed.phase, LearningPhase::Failed);
    assert_eq!(failed.mode, Some(LearningMode::Continuous));
    assert_eq!(
        failed.error.as_deref(),
        Some("continuous transition environment failed")
    );
}

#[test]
fn failure_keeps_the_active_mode_for_retry() {
    let mut controller = LearningController::default();
    controller.start(LearningMode::ByStep, "ch/unit").unwrap();

    let failed = controller.observe(&PlaybackState::Error {
        code: "command_failed".to_string(),
        message: "failed".to_string(),
        position: Some(position()),
        interruption: None,
    });

    assert_eq!(failed.phase, LearningPhase::Failed);
    assert_eq!(failed.mode, Some(LearningMode::ByStep));
    assert_eq!(failed.unit_path.as_deref(), Some("ch/unit"));
}

#[test]
fn an_error_without_a_position_still_fails_the_active_mode() {
    let mut controller = LearningController::default();
    controller.start(LearningMode::ByStep, "ch/unit").unwrap();

    let failed = controller.observe(&PlaybackState::Error {
        code: "persistence_failed".to_string(),
        message: "state database is locked".to_string(),
        position: None,
        interruption: None,
    });

    assert_eq!(failed.phase, LearningPhase::Failed);
    assert_eq!(failed.mode, Some(LearningMode::ByStep));
    assert_eq!(failed.unit_path.as_deref(), Some("ch/unit"));
    assert_eq!(failed.error.as_deref(), Some("state database is locked"));
}

#[test]
fn adapter_disconnect_ends_the_active_learning_mode() {
    for mode in [LearningMode::ByStep, LearningMode::Continuous] {
        let mut controller = LearningController::default();
        controller.start(mode, "ch/unit").unwrap();

        let stopped = controller.observe(&PlaybackState::Paused {
            position: position(),
            interruption: crate::PlaybackInterruption {
                reason: crate::PlaybackInterruptionReason::AdapterDisconnected,
                exit_code: None,
                output: None,
                output_truncated: None,
            },
        });

        assert_eq!(stopped.phase, LearningPhase::Idle);
        assert_eq!(stopped.mode, None);
        assert_eq!(stopped.unit_path, None);
        assert_eq!(stopped.error, None);
    }
}
