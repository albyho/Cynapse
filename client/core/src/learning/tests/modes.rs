use super::*;

#[test]
fn by_step_learning_waits_at_a_step_boundary_and_stops_without_losing_mode_state() {
    let mut controller = LearningController::default();
    let started = controller.start(LearningMode::ByStep, "ch/unit").unwrap();
    assert_eq!(started.phase, LearningPhase::Running);
    assert_eq!(started.mode, Some(LearningMode::ByStep));

    let waiting = controller.observe(&PlaybackState::StepComplete {
        position: position(),
        continuation: None::<NarrationClipContinuation>,
        requires_human: false,
    });
    assert_eq!(waiting.phase, LearningPhase::WaitingForStep);
    assert_eq!(waiting.unit_path.as_deref(), Some("ch/unit"));

    let stopped = controller.stop();
    assert_eq!(stopped.phase, LearningPhase::Idle);
    assert_eq!(stopped.mode, None);
    assert_eq!(stopped.unit_path, None);
}

#[test]
fn completion_finishes_the_active_learning_mode() {
    let mut controller = LearningController::default();
    controller.start(LearningMode::ByStep, "ch/unit").unwrap();

    let completed = controller.observe(&PlaybackState::UnitComplete {
        position: position(),
    });

    assert_eq!(completed.phase, LearningPhase::Completed);
    assert_eq!(completed.mode, None);
}

#[test]
fn continuous_learning_is_reported_as_a_distinct_active_mode() {
    let mut controller = LearningController::default();

    let started = controller
        .start(LearningMode::Continuous, "ch/unit")
        .unwrap();

    assert_eq!(started.mode, Some(LearningMode::Continuous));
    assert_eq!(started.phase, LearningPhase::Running);
}

#[test]
fn continuous_learning_continues_through_step_and_unit_boundaries() {
    let mut controller = LearningController::default();
    controller
        .start(LearningMode::Continuous, "ch/unit")
        .unwrap();

    let step_complete = controller.observe(&PlaybackState::StepComplete {
        position: position(),
        continuation: None::<NarrationClipContinuation>,
        requires_human: false,
    });
    assert_eq!(step_complete.phase, LearningPhase::Running);
    assert_eq!(step_complete.mode, Some(LearningMode::Continuous));

    let unit_complete = controller.observe(&PlaybackState::UnitComplete {
        position: position(),
    });
    assert_eq!(unit_complete.phase, LearningPhase::Running);
    assert_eq!(unit_complete.mode, Some(LearningMode::Continuous));
}

#[test]
fn continuous_learning_waits_at_a_human_step() {
    let mut controller = LearningController::default();
    controller
        .start(LearningMode::Continuous, "ch/unit")
        .unwrap();

    let waiting = controller.observe(&PlaybackState::StepComplete {
        position: position(),
        continuation: None::<NarrationClipContinuation>,
        requires_human: true,
    });

    assert_eq!(waiting.phase, LearningPhase::WaitingForStep);
    assert_eq!(waiting.mode, Some(LearningMode::Continuous));
    assert_eq!(waiting.unit_path.as_deref(), Some("ch/unit"));
}
