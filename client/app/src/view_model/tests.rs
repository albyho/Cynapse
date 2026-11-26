use super::{
    NextUnitOrigin, adapter_status_for, apply_advance_failure, awaiting_human_state,
    breadcrumb_titles, continuous_step_auto_advances, course_identity_matches,
    interruption_is_passive, learner_action_needed, learner_action_pending, learning_launch_label,
    passive_wait_needs_learner, retry_advance_after_failure, should_stop_narration,
    start_controls_visible, step_advance_label, transition_blocks_controls, tree_actions_available,
    unit_progress_summary, visible_playback_position,
};
use cynapse_client_core::{
    AdapterId, ChallengeStatus, ChallengeView, ClientTransitionKind, CourseTreeNodeView,
    LearningMode, LearningPhase, PlaybackInterruption, PlaybackInterruptionReason,
    PlaybackPosition, PlaybackState, StepView, UnitStatus, UnitView, Warning,
};
use cynapse_client_i18n::t;
use cynapse_client_model::{
    ClientModel, ClientSettings, ConnectionState, NextUnitDecision, NextUnitTarget,
    UnitProgressKind, UnitRow, next_unit_after, next_unit_decision, prompted_next_unit,
    unit_progress_kind,
};

fn unit(path: &str, status: UnitStatus) -> UnitRow {
    UnitRow {
        path: path.to_string(),
        title: path.to_string(),
        status,
        adapters: vec![AdapterId::Vscode],
    }
}

fn connected(adapter: &str) -> ConnectionState {
    ConnectionState::Connected {
        adapter: adapter.to_string(),
    }
}

fn step(id: &str) -> StepView {
    StepView {
        step_id: id.to_string(),
        title: id.to_string(),
        challenge: None,
        challenge_status: ChallengeStatus::NotStarted,
        challenge_attempts: 0,
        started: false,
        completed: false,
        requires_human: false,
    }
}

fn unit_view(steps: Vec<StepView>) -> UnitView {
    UnitView {
        path: "ch/unit".to_string(),
        title: "Unit".to_string(),
        steps,
        current_step_id: None,
        adapters: vec![AdapterId::Vscode],
        status: UnitStatus::InProgress,
        completed: false,
        skipped_challenges: 1,
    }
}

#[test]
fn stale_playback_position_is_hidden_outside_the_active_selected_unit() {
    let position = PlaybackPosition {
        unit_path: "ch/unit".into(),
        step_title: "Step".into(),
        ..Default::default()
    };
    let playing = PlaybackState::Playing {
        action_id: "action".into(),
        action_label: "Action".into(),
        position: position.clone(),
    };

    assert!(visible_playback_position(&playing, true, Some("ch/unit")).is_some());
    assert!(visible_playback_position(&playing, false, Some("ch/unit")).is_none());
    assert!(visible_playback_position(&playing, true, None).is_none());
    assert!(visible_playback_position(&playing, true, Some("ch/other")).is_none());

    let paused = PlaybackState::Paused {
        position,
        interruption: PlaybackInterruption {
            reason: PlaybackInterruptionReason::UserStop,
            exit_code: None,
            output: None,
            output_truncated: None,
        },
    };
    assert!(visible_playback_position(&paused, false, Some("ch/unit")).is_none());
}

#[test]
fn breadcrumb_includes_the_whole_unit_ancestor_chain() {
    fn node(path: &str, title: &str, children: Vec<CourseTreeNodeView>) -> CourseTreeNodeView {
        CourseTreeNodeView {
            path: path.to_string(),
            title: title.to_string(),
            completed: false,
            unit: None,
            children,
        }
    }

    let tree = vec![node(
        "ch",
        "Chapter",
        vec![node(
            "ch/section",
            "Section",
            vec![node("ch/section/unit", "Unit", Vec::new())],
        )],
    )];

    assert_eq!(
        breadcrumb_titles(&tree, "ch/section/unit"),
        vec!["Chapter", "Section", "Unit"]
    );
    assert_eq!(
        breadcrumb_titles(&tree, "ch/section"),
        vec!["Chapter", "Section"]
    );
    assert_eq!(breadcrumb_titles(&tree, "ch"), vec!["Chapter"]);
    assert!(breadcrumb_titles(&tree, "missing").is_empty());
}

#[test]
fn awaiting_human_covers_only_human_step_completion() {
    let position = |step_id: &str| PlaybackPosition {
        unit_path: "ch/unit".to_string(),
        step_id: step_id.to_string(),
        ..PlaybackPosition::default()
    };

    assert!(awaiting_human_state(&PlaybackState::StepComplete {
        position: position("human"),
        continuation: None,
        requires_human: true,
    }));
    assert!(!awaiting_human_state(&PlaybackState::StepComplete {
        position: position("ordinary"),
        continuation: None,
        requires_human: false,
    }));
    assert!(!awaiting_human_state(&PlaybackState::UnitComplete {
        position: position("human"),
    }));
    assert!(!awaiting_human_state(&PlaybackState::Idle));
}

#[test]
fn pending_actions_clear_learner_topmost() {
    assert!(learner_action_pending(true, false, false, false));
    assert!(learner_action_pending(false, true, false, false));
    assert!(learner_action_pending(false, false, true, false));
    assert!(learner_action_pending(false, false, false, true));
    assert!(!learner_action_pending(false, false, false, false));
}

#[test]
fn learner_action_covers_human_next_retry_prompt_and_passive_waits() {
    assert!(learner_action_needed(
        true, false, false, false, false, false
    ));
    assert!(learner_action_needed(
        false, true, false, false, false, false
    ));
    assert!(learner_action_needed(
        false, false, true, false, false, false
    ));
    assert!(learner_action_needed(
        false, false, false, true, false, false
    ));
    assert!(learner_action_needed(
        false, false, false, false, true, false
    ));
    assert!(learner_action_needed(
        false, false, false, false, false, true
    ));
    assert!(!learner_action_needed(
        false, false, false, false, false, false
    ));
}

#[test]
fn passive_interruptions_need_learner_action() {
    for reason in [
        PlaybackInterruptionReason::AdapterDisconnected,
        PlaybackInterruptionReason::Timeout,
        PlaybackInterruptionReason::CommandFailure,
        PlaybackInterruptionReason::MediaFailure,
        PlaybackInterruptionReason::ForcedTermination,
        PlaybackInterruptionReason::JupyterLabKernelRestarted,
    ] {
        assert!(interruption_is_passive(reason), "{reason:?}");
    }
    for reason in [
        PlaybackInterruptionReason::UserStop,
        PlaybackInterruptionReason::ClientExit,
        PlaybackInterruptionReason::CourseSwitch,
    ] {
        assert!(!interruption_is_passive(reason), "{reason:?}");
    }
}

#[test]
fn passive_wait_needs_learner_for_errors_and_disconnected_adapters() {
    let step_complete = PlaybackState::StepComplete {
        position: PlaybackPosition::default(),
        continuation: None,
        requires_human: false,
    };
    let error = PlaybackState::Error {
        code: "E_TEST".to_string(),
        message: "boom".to_string(),
        position: None,
        interruption: None,
    };
    let paused = |reason| PlaybackState::Paused {
        position: PlaybackPosition::default(),
        interruption: PlaybackInterruption {
            reason,
            exit_code: None,
            output: None,
            output_truncated: None,
        },
    };
    assert!(passive_wait_needs_learner(false, &step_complete));
    assert!(!passive_wait_needs_learner(true, &step_complete));
    assert!(passive_wait_needs_learner(true, &error));
    assert!(passive_wait_needs_learner(
        false,
        &paused(PlaybackInterruptionReason::AdapterDisconnected)
    ));
    assert!(!passive_wait_needs_learner(
        false,
        &paused(PlaybackInterruptionReason::UserStop)
    ));
    assert!(!passive_wait_needs_learner(
        false,
        &PlaybackState::UnitComplete {
            position: PlaybackPosition::default(),
        }
    ));
    assert!(!passive_wait_needs_learner(false, &PlaybackState::Idle));
    assert!(!passive_wait_needs_learner(
        false,
        &PlaybackState::Ready { position: None }
    ));
}

#[test]
fn challenge_steps_auto_advance_even_when_they_require_human() {
    assert!(continuous_step_auto_advances(false, false));
    assert!(continuous_step_auto_advances(false, true));
    assert!(continuous_step_auto_advances(true, true));
    assert!(!continuous_step_auto_advances(true, false));
}

#[test]
fn unit_progress_summary_counts_challenges_and_steps() {
    let steps = |pending_started: bool| {
        let challenge = || ChallengeView {
            instructions: "do it".into(),
            hint: None,
            allow_skip: true,
        };
        let mut passed = step("a");
        passed.challenge = Some(challenge());
        passed.challenge_status = ChallengeStatus::Passed;
        passed.completed = true;
        let mut skipped = step("b");
        skipped.challenge = Some(challenge());
        skipped.challenge_status = ChallengeStatus::Skipped;
        skipped.completed = true;
        let mut pending = step("c");
        pending.started = pending_started;
        vec![passed, skipped, pending]
    };

    let summary = unit_progress_summary(&unit_view(steps(false)));
    assert_eq!(summary.challenges_total, 2);
    assert_eq!(summary.challenges_passed, 1);
    assert_eq!(summary.challenges_skipped, 1);
    assert_eq!(summary.steps_total, 3);
    assert_eq!(summary.steps_completed, 2);
    assert_eq!(summary.steps_in_progress, 0);
    assert_eq!(summary.in_progress_step, None);

    let summary = unit_progress_summary(&unit_view(steps(true)));
    assert_eq!(summary.steps_completed, 2);
    assert_eq!(summary.steps_in_progress, 1);
    assert_eq!(summary.in_progress_step, Some(3));
    assert_eq!(
        summary.steps_completed + summary.steps_in_progress,
        summary.steps_total,
        "the displayed numerator counts the in-progress Step too"
    );
}

#[test]
fn unit_progress_summary_counts_every_in_progress_step_and_reports_the_first() {
    let mut first = step("a");
    first.started = true;
    let mut second = step("b");
    second.started = true;

    let summary = unit_progress_summary(&unit_view(vec![first, second]));
    assert_eq!(summary.steps_total, 2);
    assert_eq!(summary.steps_in_progress, 2);
    assert_eq!(summary.in_progress_step, Some(1));
}

#[test]
fn a_committed_plan_failure_stops_learning_but_pre_commit_failures_keep_retry() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _) =
        ClientModel::new_at(ClientSettings::default(), directory.path().join("state.db")).unwrap();

    model
        .start_learning(LearningMode::Continuous, "ch/unit")
        .unwrap();
    apply_advance_failure(&mut model, &Warning::NextUnitPlanUnavailable);
    assert!(!model.is_learning());

    model
        .start_learning(LearningMode::Continuous, "ch/unit")
        .unwrap();
    apply_advance_failure(&mut model, &Warning::Message("env failed".into()));
    assert!(model.is_learning());
    assert_eq!(model.state().learning().phase, LearningPhase::Failed);

    model.stop_learning_local();
    model
        .start_learning(LearningMode::Continuous, "ch/unit")
        .unwrap();
    apply_advance_failure(&mut model, &Warning::AdapterDisconnected);
    assert!(model.is_learning());
    assert_eq!(model.state().learning().phase, LearningPhase::Running);
}

#[gpui_kit::test]
fn formula_source_fallback_is_limited_to_the_current_caption(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::AppContext;

    let directory = tempfile::tempdir().unwrap();
    let (model, events) =
        ClientModel::new_at(ClientSettings::default(), directory.path().join("state.db")).unwrap();
    cx.update(|cx| {
        let view_model =
            cx.new(|cx| super::ClientViewModel::new_without_narration(model, events, cx));
        view_model.update(cx, |view_model, cx| {
            let first = r"观察 \(x\)，再继续。";
            let next = r"观察 \(y\)，再继续。";
            view_model.set_caption(Some(first.into()), cx);
            view_model.show_caption_formula_source(first, cx);
            assert!(!view_model.caption_formulas_enabled());
            assert_eq!(view_model.caption_text(), first);

            view_model.set_caption(Some(next.into()), cx);
            view_model.show_caption_formula_source(first, cx);
            assert!(view_model.caption_formulas_enabled());
            assert_eq!(view_model.caption_text(), next);
        });
    });
}

#[gpui_kit::test]
fn caption_card_fits_precalculated_window(cx: &mut gpui_kit::TestAppContext) {
    use crate::{platform, views::caption::CaptionView};
    use cynapse_client_core::CaptionPosition;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, px, size};

    let directory = tempfile::tempdir().unwrap();
    let mut settings = ClientSettings::default();
    settings.caption.background_color = Some("#dd2020".into());
    let (model, events) = ClientModel::new_at(settings, directory.path().join("state.db")).unwrap();
    let view_model = cx
        .update(|cx| cx.new(|cx| super::ClientViewModel::new_without_narration(model, events, cx)));
    let main = cx.open_window(size(px(500.), px(500.)), |_, _| gpui_kit::Empty);
    let overlay = cx.open_window(size(px(500.), px(200.)), |_, cx| {
        CaptionView::new(view_model.clone(), cx)
    });
    cx.update_window(overlay.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("caption-card").is_none());
    })
    .unwrap();

    for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
        for window in [main.into(), overlay.into()] {
            cx.simulate_window_scale_factor_change(window, scale);
        }
        for (position, text, single_line) in [
            (CaptionPosition::Top, "第 1 章《Hello World》。", Some(true)),
            (
                CaptionPosition::Bottom,
                "第 1111111111111111111 章《Hello World》",
                None,
            ),
            (
                CaptionPosition::Bottom,
                concat!(
                    "Rust <c.aqua>类型系统</c>会根据上下文推断类型。",
                    "这是一条需要换行的长字幕，用来确认窗口高度能容纳完整内容。",
                    "这是一条需要换行的长字幕，用来确认窗口高度能容纳完整内容。",
                    "字幕的红色背景和圆角也应该完整显示。"
                ),
                Some(false),
            ),
        ] {
            let (width, height) = cx.update(|cx| {
                view_model.update(cx, |view_model, cx| {
                    view_model.set_caption_position(position, cx);
                    view_model.set_caption(Some(text.into()), cx);
                });
                platform::measure_caption_size(cx, main.into(), &view_model);
                let view_model = view_model.read(cx);
                (view_model.caption_width(), view_model.caption_height())
            });
            let native_size = |dimension: u32| px((dimension as f32 * scale).round() / scale);
            cx.simulate_window_resize(
                overlay.into(),
                size(native_size(width), native_size(height)),
            );
            cx.update_window(overlay.into(), |_, window, cx| {
                window.render_frame(cx);
                let bounds = window.find("caption-card").bounds();
                assert!(
                    bounds.top() >= px(0.) && bounds.bottom() <= window.viewport_size().height,
                    "{position:?} caption card {bounds:?} exceeds {width} × {height} at scale {scale}"
                );
                if let Some(single_line) = single_line {
                    let line_height = window.pixel_snap(px(32. * 1.3));
                    assert_eq!(
                        (bounds.size.height - line_height - px(16.)).abs() < px(1.),
                        single_line,
                        "unexpected line count at scale {scale} for {text:?}, got {bounds:?}"
                    );
                }
            })
            .unwrap();
        }
    }
}

#[test]
fn narration_tail_survives_step_completion_while_media_runs() {
    let completed = PlaybackState::StepComplete {
        position: PlaybackPosition::default(),
        continuation: None,
        requires_human: false,
    };
    assert!(
        !should_stop_narration(&completed, true, false),
        "a running tail keeps playing after the Step completes"
    );
    assert!(
        should_stop_narration(&completed, false, false),
        "no tail and no continuation means nothing to keep"
    );
    assert!(
        !should_stop_narration(
            &PlaybackState::UnitComplete {
                position: PlaybackPosition::default()
            },
            false,
            false
        ),
        "unit completion does not cut an active tail"
    );
    assert!(should_stop_narration(&PlaybackState::Idle, true, false));
}

#[test]
fn advancing_a_step_stops_the_old_narration_tail() {
    let playing = PlaybackState::Playing {
        action_id: "action".into(),
        action_label: "Action".into(),
        position: PlaybackPosition::default(),
    };
    assert!(should_stop_narration(&playing, true, true));
    assert!(!should_stop_narration(&playing, true, false));
}

#[test]
fn no_adapter_needed_and_no_connection_shows_a_muted_indicator() {
    let status = adapter_status_for(&[], &ConnectionState::WaitingForAdapter);
    assert!(!status.connected);
    assert!(!status.required);
    assert_eq!(status.tooltip, t("adapter.disconnected"));
}

#[test]
fn no_adapter_needed_but_connected_shows_green() {
    let status = adapter_status_for(&[], &connected("vscode"));
    assert!(status.connected);
    assert!(!status.required);
    assert_eq!(status.tooltip, t("adapter.connected"));
}

#[test]
fn required_adapter_connected_shows_green() {
    let status = adapter_status_for(&[AdapterId::Vscode], &connected("vscode"));
    assert!(status.connected);
    assert!(status.required);
}

#[test]
fn required_adapter_missing_shows_waiting() {
    let status = adapter_status_for(&[AdapterId::Vscode], &ConnectionState::WaitingForAdapter);
    assert!(!status.connected);
    assert!(status.required);
    assert_eq!(status.tooltip, t("adapter.waiting"));
}

#[test]
fn required_adapter_connected_to_something_else_shows_waiting() {
    let status = adapter_status_for(&[AdapterId::Vscode], &connected("other"));
    assert!(!status.connected);
    assert!(status.required);
}

#[test]
fn leaf_progress_distinguishes_new_partial_and_completed() {
    assert_eq!(
        unit_progress_kind(&unit("new", UnitStatus::NotStarted)),
        UnitProgressKind::New
    );
    assert_eq!(
        unit_progress_kind(&unit("partial", UnitStatus::InProgress)),
        UnitProgressKind::Partial
    );
    assert_eq!(
        unit_progress_kind(&unit("paused", UnitStatus::Paused)),
        UnitProgressKind::Partial
    );
    assert_eq!(
        unit_progress_kind(&unit("completed", UnitStatus::Completed)),
        UnitProgressKind::Completed
    );
}

#[test]
fn next_unit_searches_forward_including_completed_units() {
    let units = [
        unit("branch/first", UnitStatus::InProgress),
        unit("branch/second", UnitStatus::Completed),
        unit("branch/third", UnitStatus::NotStarted),
    ];

    assert_eq!(
        next_unit_after(&units, "branch/first").map(|unit| unit.path.as_str()),
        Some("branch/second")
    );
    assert_eq!(
        next_unit_after(&units, "branch/third").map(|unit| unit.path.as_str()),
        None
    );
}

#[test]
fn next_unit_decision_prompts_before_touching_a_studied_unit() {
    let units = [
        unit("branch/first", UnitStatus::Completed),
        unit("branch/not-started", UnitStatus::NotStarted),
        unit("branch/paused", UnitStatus::Paused),
        unit("branch/in-progress", UnitStatus::InProgress),
        unit("branch/completed", UnitStatus::Completed),
    ];

    assert_eq!(
        next_unit_decision(&units, "branch/first"),
        NextUnitDecision::Select(&units[1])
    );
    assert_eq!(
        next_unit_decision(&units, "branch/not-started"),
        NextUnitDecision::PromptContinueOrReset(&units[2])
    );
    assert_eq!(
        next_unit_decision(&units, "branch/paused"),
        NextUnitDecision::PromptContinueOrReset(&units[3])
    );
    assert_eq!(
        next_unit_decision(&units, "branch/in-progress"),
        NextUnitDecision::PromptReset(&units[4])
    );
    assert_eq!(
        next_unit_decision(&units, "branch/completed"),
        NextUnitDecision::NoNextUnit
    );
}

#[test]
fn prompted_next_unit_only_blocks_for_partial_or_completed_targets() {
    let units = [
        unit("branch/first", UnitStatus::InProgress),
        unit("branch/second", UnitStatus::InProgress),
        unit("branch/third", UnitStatus::Completed),
        unit("branch/fourth", UnitStatus::NotStarted),
    ];

    assert_eq!(
        prompted_next_unit(&units, "branch/first").map(|unit| unit.path.as_str()),
        Some("branch/second")
    );
    assert_eq!(
        prompted_next_unit(&units, "branch/second").map(|unit| unit.path.as_str()),
        Some("branch/third")
    );
    assert!(prompted_next_unit(&units, "branch/third").is_none());
}

#[test]
fn next_unit_prompt_uses_the_target_source_not_the_playback_position() {
    let units = [
        unit("branch/first", UnitStatus::Completed),
        unit("branch/second", UnitStatus::InProgress),
        unit("branch/third", UnitStatus::Completed),
        unit("branch/fourth", UnitStatus::NotStarted),
    ];

    let cursor = NextUnitTarget {
        path: "branch/fourth".to_string(),
        source: "branch/third".to_string(),
        origin: NextUnitOrigin::Cursor,
    };
    assert!(cursor.prompted_unit(&units).is_none());

    let boundary = NextUnitTarget {
        path: "branch/fourth".to_string(),
        source: "branch/first".to_string(),
        origin: NextUnitOrigin::Boundary,
    };
    assert_eq!(
        boundary
            .prompted_unit(&units)
            .map(|unit| unit.path.as_str()),
        Some("branch/second")
    );
}

#[test]
fn step_advance_label_tracks_step_and_unit_boundaries() {
    let mut position = PlaybackPosition {
        unit_step_index: 0,
        unit_step_total: 2,
        ..Default::default()
    };

    assert_eq!(step_advance_label(&position, true), Some(t("next.step")));
    assert_eq!(step_advance_label(&position, false), Some(t("next.step")));

    position.unit_step_index = 1;
    assert_eq!(step_advance_label(&position, true), Some(t("next.unit")));
    assert_eq!(step_advance_label(&position, false), None);
}

#[test]
fn start_controls_are_hidden_while_learning_or_at_a_unit_boundary() {
    assert!(start_controls_visible(false, false));
    assert!(!start_controls_visible(true, false));
    assert!(!start_controls_visible(false, true));
}

#[test]
fn tree_actions_are_available_only_outside_active_playback_and_learning() {
    assert!(tree_actions_available(false, false));
    assert!(!tree_actions_available(true, false));
    assert!(!tree_actions_available(false, true));
}

#[test]
fn selecting_a_unit_does_not_block_controls() {
    assert!(!transition_blocks_controls(ClientTransitionKind::Selecting));
    assert!(transition_blocks_controls(ClientTransitionKind::Resetting));
    assert!(transition_blocks_controls(ClientTransitionKind::Starting));
}

#[test]
fn learning_launch_labels_follow_main_and_completed_units_have_no_launch() {
    assert_eq!(
        learning_launch_label(UnitStatus::NotStarted),
        Some(t("unit.start"))
    );
    assert_eq!(
        learning_launch_label(UnitStatus::InProgress),
        Some(t("unit.continue"))
    );
    assert_eq!(
        learning_launch_label(UnitStatus::Paused),
        Some(t("unit.continue"))
    );
    assert_eq!(learning_launch_label(UnitStatus::Completed), None);
}

#[test]
fn stale_course_identity_does_not_match() {
    let identity = ("course-id".to_string(), "course".to_string());

    assert!(course_identity_matches(Some(&identity), &identity));
    assert!(!course_identity_matches(None, &identity));
    assert!(!course_identity_matches(
        Some(&("other-id".to_string(), "course".to_string())),
        &identity
    ));
}

#[test]
fn failed_continuous_advance_can_retry_at_a_unit_boundary() {
    assert!(retry_advance_after_failure(LearningPhase::Failed, true));
    assert!(!retry_advance_after_failure(LearningPhase::Running, true));
    assert!(!retry_advance_after_failure(LearningPhase::Failed, false));
}
