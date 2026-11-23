//! PlaybackSession progression and failure decisions.

use super::super::progression::{ActionAdvance, plan_after_successful_action};
use super::super::*;
use super::support::*;

fn empty_session() -> PlaybackSession {
    PlaybackSession {
        cursor: None,
        state: PlaybackState::Idle,
        pending_boundary: None,
        correlation_id: None,
        skip_waits: false,
        skip_waits_through: None,
        force_skip_waits: false,
        skip_unit_final_wait: false,
    }
}

fn interruption(reason: PlaybackInterruptionReason) -> PlaybackInterruption {
    PlaybackInterruption {
        reason,
        exit_code: None,
        output: None,
        output_truncated: None,
    }
}

#[test]
fn action_advance_plan_covers_dispatch_and_boundaries() {
    let steps = vec![
        playback_step_in_unit(
            "first_unit",
            "First Unit",
            "first",
            "First",
            vec![insert_text("a"), insert_text("b")],
        ),
        playback_step_in_unit(
            "first_unit",
            "First Unit",
            "second",
            "Second",
            vec![insert_text("c")],
        ),
        playback_step_in_unit(
            "second_unit",
            "Second Unit",
            "third",
            "Third",
            vec![insert_text("d")],
        ),
    ];
    let cursor = |step_index, action_index| PlaybackCursor {
        step_index,
        action_index,
    };

    assert_eq!(
        plan_after_successful_action(&steps, cursor(0, 0)),
        ActionAdvance::Dispatch(cursor(0, 1))
    );
    assert_eq!(
        plan_after_successful_action(&steps, cursor(0, 1)),
        ActionAdvance::Boundary(PendingBoundary::Step {
            completed: cursor(0, 2),
            next: cursor(1, 0),
        })
    );
    assert_eq!(
        plan_after_successful_action(&steps, cursor(1, 0)),
        ActionAdvance::Boundary(PendingBoundary::Unit {
            completed: cursor(1, 1),
        })
    );
    assert_eq!(
        plan_after_successful_action(&steps, cursor(2, 0)),
        ActionAdvance::Boundary(PendingBoundary::Unit {
            completed: cursor(2, 1),
        })
    );
}

#[test]
fn interrupted_action_pauses_and_restarts_the_step() {
    let steps = vec![playback_step(
        "step",
        "Step",
        vec![insert_text("a"), insert_text("b")],
    )];
    let index = PlaybackIndex::new(&steps);
    let cursor = PlaybackCursor {
        step_index: 0,
        action_index: 1,
    };
    let mut session = empty_session();
    session.state = PlaybackState::Playing {
        action_id: "action".to_string(),
        action_label: "Action".to_string(),
        position: PlaybackPosition::default(),
    };

    let state = session.interrupted_action(
        cursor,
        &steps,
        &index,
        interruption(PlaybackInterruptionReason::UserStop),
        None,
    );

    assert!(matches!(state, PlaybackState::Paused { .. }));
    assert_eq!(
        session.cursor,
        Some(PlaybackCursor {
            step_index: 0,
            action_index: 0
        })
    );
    assert_eq!(session.pending_boundary, None);
}

#[test]
fn interrupted_action_reports_a_timeout_as_an_error() {
    let steps = vec![playback_step("step", "Step", vec![insert_text("a")])];
    let index = PlaybackIndex::new(&steps);
    let cursor = PlaybackCursor {
        step_index: 0,
        action_index: 0,
    };
    let mut session = empty_session();

    let state = session.interrupted_action(
        cursor,
        &steps,
        &index,
        interruption(PlaybackInterruptionReason::Timeout),
        Some(("action_timeout".to_string(), "too slow".to_string())),
    );

    assert!(matches!(
        state,
        PlaybackState::Error {
            code,
            message,
            interruption: Some(PlaybackInterruption {
                reason: PlaybackInterruptionReason::Timeout,
                ..
            }),
            ..
        } if code == "action_timeout" && message == "too slow"
    ));
    assert_eq!(
        session.cursor,
        Some(PlaybackCursor {
            step_index: 0,
            action_index: 0
        })
    );
}

#[test]
fn paused_interruption_reasons_are_preserved() {
    let steps = vec![playback_step("step", "Step", vec![insert_text("a")])];
    let index = PlaybackIndex::new(&steps);
    let cursor = PlaybackCursor {
        step_index: 0,
        action_index: 0,
    };

    for reason in [
        PlaybackInterruptionReason::UserStop,
        PlaybackInterruptionReason::CommandFailure,
        PlaybackInterruptionReason::JupyterLabKernelRestarted,
    ] {
        let mut session = empty_session();
        let state = session.interrupted_action(cursor, &steps, &index, interruption(reason), None);

        assert!(matches!(
            state,
            PlaybackState::Paused { interruption, .. } if interruption.reason == reason
        ));
    }
}

#[test]
fn failed_action_clears_the_boundary_and_retries_the_step() {
    let mut session = empty_session();
    session.cursor = Some(PlaybackCursor {
        step_index: 1,
        action_index: 1,
    });
    session.pending_boundary = Some(PendingBoundary::Unit {
        completed: PlaybackCursor {
            step_index: 1,
            action_index: 2,
        },
    });

    let state = session.failed_action(
        Some(PlaybackCursor {
            step_index: 1,
            action_index: 1,
        }),
        Some(PlaybackPosition::default()),
        "failed".to_string(),
        "message".to_string(),
        Some(interruption(PlaybackInterruptionReason::MediaFailure)),
    );

    assert!(matches!(
        state,
        PlaybackState::Error {
            interruption: Some(PlaybackInterruption {
                reason: PlaybackInterruptionReason::MediaFailure,
                ..
            }),
            ..
        }
    ));
    assert_eq!(
        session.cursor,
        Some(PlaybackCursor {
            step_index: 1,
            action_index: 0
        })
    );
    assert_eq!(session.pending_boundary, None);
}
