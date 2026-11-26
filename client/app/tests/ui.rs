//! Headless GPUI control-surface tests.
//!
//! `open_test_window_with` builds a `ClientModel`/`ClientViewModel` and applies
//! Course state before rendering; use it for visible Start/Stop/status assertions.

use cynapse_client::view_model::ClientViewModel;
use cynapse_client::views::course_import_dialog::CourseImportDialogView;
use cynapse_client::views::main::MainView;
use cynapse_client_core::{
    CaptionPosition, ChallengeStatus, ChallengeView, ConnectionState, CourseSessionState,
    CourseTreeNodeView, LearningMode, LearningPhase, LearningView, PlaybackInterruption,
    PlaybackInterruptionReason, PlaybackPosition, PlaybackState, StepView, UnitStatus, UnitView,
};
use cynapse_client_i18n::t;
use cynapse_client_model::{ClientEvent, ClientModel, ClientSettings};
use gpui_kit::base::Root;
use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Entity, TestAppContext, px, size};
use std::path::PathBuf;

fn open_test_window(cx: &mut TestAppContext) -> (gpui_kit::WindowHandle<Root>, tempfile::TempDir) {
    open_test_window_with(cx, |_, _| {})
}

fn open_test_window_with(
    cx: &mut TestAppContext,
    prepare: impl FnOnce(&mut ClientModel, &mut CourseSessionState),
) -> (gpui_kit::WindowHandle<Root>, tempfile::TempDir) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    open_test_window_with_package(cx, package, prepare)
}

fn open_test_window_with_package(
    cx: &mut TestAppContext,
    package: PathBuf,
    prepare: impl FnOnce(&mut ClientModel, &mut CourseSessionState),
) -> (gpui_kit::WindowHandle<Root>, tempfile::TempDir) {
    let (window, _, directory) = open_test_window_parts(cx, package, prepare);
    (window, directory)
}

fn open_test_window_parts(
    cx: &mut TestAppContext,
    package: PathBuf,
    prepare: impl FnOnce(&mut ClientModel, &mut CourseSessionState),
) -> (
    gpui_kit::WindowHandle<Root>,
    Entity<ClientViewModel>,
    tempfile::TempDir,
) {
    open_test_window_parts_after(cx, package, prepare, |_| {})
}

fn open_test_window_parts_after(
    cx: &mut TestAppContext,
    package: PathBuf,
    prepare: impl FnOnce(&mut ClientModel, &mut CourseSessionState),
    after: impl FnOnce(&mut ClientModel),
) -> (
    gpui_kit::WindowHandle<Root>,
    Entity<ClientViewModel>,
    tempfile::TempDir,
) {
    cx.update(gpui_kit::init);
    let directory = tempfile::tempdir().expect("temp dir");
    let (mut model, mut events) =
        ClientModel::new_at(ClientSettings::default(), directory.path().join("state.db"))
            .expect("test model");
    // The UI tests inject events by hand; a real local adapter would also
    // connect and play the course from a background thread, which the
    // gpui test scheduler rejects as nondeterministic.
    model.session().suppress_local_adapter();
    let mut state = futures::executor::block_on(model.session().open_course(package))
        .expect("open test course");
    prepare(&mut model, &mut state);
    // The helper applies the prepared state directly; discard queued session
    // events so they cannot overwrite it once the view model starts consuming.
    while events.try_recv().is_ok() {}
    model.apply_course_state(state);
    after(&mut model);

    let view_model_handle = std::rc::Rc::new(std::cell::RefCell::new(None));
    let view_model_for_window = view_model_handle.clone();
    let window = cx.open_window(size(px(640.), px(900.)), move |window, cx| {
        let view_model = cx.new(|cx| ClientViewModel::new_without_narration(model, events, cx));
        *view_model_for_window.borrow_mut() = Some(view_model.clone());
        let main = cx.new(|cx| MainView::new(view_model, window, cx));
        Root::new(main, window, cx)
    });
    let view_model = view_model_handle
        .borrow()
        .clone()
        .expect("view model created");
    (window, view_model, directory)
}

#[gpui_kit::test]
fn caption_hover_flips_the_runtime_position_without_writing_settings(cx: &mut TestAppContext) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (_window, view_model, _directory) = open_test_window_parts(cx, package, |_, _| {});

    cx.update(|cx| {
        view_model.update(cx, |view_model, cx| {
            assert_eq!(view_model.caption_position(), CaptionPosition::Bottom);
            view_model.toggle_caption_edge(cx);
            assert_eq!(view_model.caption_position(), CaptionPosition::Top);
            assert_eq!(
                view_model.settings().caption.position,
                CaptionPosition::Bottom,
                "the hover toggle must not reach settings"
            );
            view_model.toggle_caption_edge(cx);
            assert_eq!(view_model.caption_position(), CaptionPosition::Bottom);
        });
    });
}

#[test]
fn course_revision_changes_only_for_course_events() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (mut model, _events) =
        ClientModel::new_at(ClientSettings::default(), directory.path().join("state.db"))
            .expect("test model");
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let state = futures::executor::block_on(model.session().open_course(package))
        .expect("open test course");

    model.apply_course_state(state.clone());
    let revision = model.course_revision();

    assert!(!model.course_tree().is_empty());
    assert!(!model.units().is_empty());
    model.apply(ClientEvent::Warnings(vec!["note".to_string().into()]));
    assert_eq!(model.course_revision(), revision);

    model.apply_course_state(state);
    assert_eq!(model.course_revision(), revision + 1);
}

fn mark_unit_in_progress(state: &mut CourseSessionState, path: &str) {
    if let Some(unit) = state
        .selected_unit
        .as_mut()
        .filter(|unit| unit.path == path)
    {
        unit.status = UnitStatus::InProgress;
    }
    fn walk(nodes: &mut [CourseTreeNodeView], path: &str) -> bool {
        for node in nodes {
            if node.path == path {
                if let Some(unit) = node.unit.as_mut() {
                    unit.status = UnitStatus::InProgress;
                }
                return true;
            }
            if walk(&mut node.children, path) {
                return true;
            }
        }
        false
    }
    if let Some(course) = state.active_course.as_mut() {
        walk(&mut course.tree, path);
    }
}

#[gpui_kit::test]
fn start_popover_shows_both_learning_modes(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("start-learning", cx);
        assert_eq!(
            window.find("step-start").label(),
            Some(t("main.mode.by_step").as_str())
        );
        assert_eq!(
            window.find("continuous-start").label(),
            Some(t("main.mode.continuous").as_str())
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn continue_popover_says_continue_for_both_modes(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |_, state| {
        mark_unit_in_progress(state, "ch_units/first_unit");
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("start-learning").label(),
            Some(t("unit.continue").as_str())
        );
        window.click("start-learning", cx);
        assert_eq!(
            window.find("step-start").label(),
            Some(t("main.mode.continue_by_step").as_str())
        );
        assert_eq!(
            window.find("continuous-start").label(),
            Some(t("main.mode.continue_continuous").as_str())
        );
    })
    .expect("main window");
}

fn clear_unit_selection(state: &mut CourseSessionState) {
    state.selected_unit = None;
}

fn mark_unit_completed(state: &mut CourseSessionState, path: &str) {
    if let Some(unit) = state
        .selected_unit
        .as_mut()
        .filter(|unit| unit.path == path)
    {
        unit.status = UnitStatus::Completed;
        unit.completed = true;
    }
    fn walk(nodes: &mut [CourseTreeNodeView], path: &str) -> bool {
        for node in nodes {
            if node.path == path {
                if let Some(unit) = node.unit.as_mut() {
                    unit.status = UnitStatus::Completed;
                    unit.completed = true;
                }
                return true;
            }
            if walk(&mut node.children, path) {
                return true;
            }
        }
        false
    }
    if let Some(course) = state.active_course.as_mut() {
        walk(&mut course.tree, path);
    }
}

fn set_unit_steps(state: &mut CourseSessionState, path: &str, steps: Vec<StepView>) {
    if let Some(unit) = state
        .selected_unit
        .as_mut()
        .filter(|unit| unit.path == path)
    {
        unit.steps = steps.clone();
    }
    fn walk(nodes: &mut [CourseTreeNodeView], path: &str, steps: &[StepView]) -> bool {
        for node in nodes {
            if node.path == path {
                if let Some(unit) = node.unit.as_mut() {
                    unit.steps = steps.to_vec();
                }
                return true;
            }
            if walk(&mut node.children, path, steps) {
                return true;
            }
        }
        false
    }
    if let Some(course) = state.active_course.as_mut() {
        walk(&mut course.tree, path, &steps);
    }
}

fn step(step_id: &str, challenge: bool, status: ChallengeStatus) -> StepView {
    StepView {
        step_id: step_id.into(),
        title: step_id.into(),
        challenge: challenge.then(|| ChallengeView {
            instructions: "do it".into(),
            hint: None,
            allow_skip: true,
        }),
        challenge_status: status,
        challenge_attempts: u32::from(challenge),
        started: true,
        completed: true,
        requires_human: false,
    }
}

fn open_challenge_window(
    cx: &mut TestAppContext,
    status: ChallengeStatus,
) -> (gpui_kit::WindowHandle<Root>, tempfile::TempDir) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (window, _view_model, directory) = open_test_window_parts_after(
        cx,
        package,
        |_, state| {
            set_unit_steps(
                state,
                "ch_units/first_unit",
                vec![step("challenge", true, status)],
            );
        },
        |model| {
            model.apply(ClientEvent::Playback(Box::new(
                PlaybackState::StepComplete {
                    position: PlaybackPosition {
                        unit_path: "ch_units/first_unit".into(),
                        step_id: "challenge".into(),
                        ..Default::default()
                    },
                    continuation: None,
                    requires_human: true,
                },
            )));
            model.apply(ClientEvent::Learning(LearningView {
                mode: Some(LearningMode::ByStep),
                phase: LearningPhase::WaitingForStep,
                unit_path: Some("ch_units/first_unit".into()),
                error: None,
            }));
        },
    );
    (window, directory)
}

#[gpui_kit::test]
fn stopping_a_challenge_hides_the_challenge_panel(cx: &mut TestAppContext) {
    let (window, _directory) = open_challenge_window(cx, ChallengeStatus::NotStarted);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("check-challenge").is_some(),
            "the Challenge actions must be visible"
        );
        assert!(window.try_find("stop-learning").is_some());
        window.click("stop-learning", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("check-challenge").is_none(),
            "Stop must leave the Challenge state"
        );
        assert!(
            window.try_find("start-learning").is_some(),
            "Stop must return to Start/Continue"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn cancelling_the_skip_challenge_popover_keeps_the_challenge_pending(cx: &mut TestAppContext) {
    let (window, _directory) = open_challenge_window(cx, ChallengeStatus::NotStarted);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("check-challenge").is_some());
        window.click("skip-challenge", cx);
        assert_eq!(
            window.find("skip-challenge-cancel").label(),
            Some(t("common.cancel").as_str())
        );
        window.click("skip-challenge-cancel", cx);
        assert!(
            window.try_find("skip-challenge").is_some(),
            "cancelling skip must leave the challenge pending"
        );
        assert!(window.try_find("skip-challenge-cancel").is_none());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn failed_challenge_offers_check_again(cx: &mut TestAppContext) {
    let (window, _directory) = open_challenge_window(cx, ChallengeStatus::Failed);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("check-challenge").label(),
            Some(t("challenge.check_again").as_str())
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn finished_challenges_hide_the_action_buttons(cx: &mut TestAppContext) {
    let (window, _directory) = open_challenge_window(cx, ChallengeStatus::Passed);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("check-challenge").is_none());
        assert!(window.try_find("skip-challenge").is_none());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn startup_overlay_checks_environment_before_marking_workspace_ready(cx: &mut TestAppContext) {
    use cynapse_client_core::{
        ClientTransitionKind, ClientTransitionPhase, ClientTransitionStage, ClientTransitionView,
    };
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (_window, model, _directory) = open_test_window_parts_after(
        cx,
        package,
        |_, _| {},
        |model| {
            model.apply(ClientEvent::Transition(Some(ClientTransitionView {
                id: "start".into(),
                kind: ClientTransitionKind::Starting,
                phase: ClientTransitionPhase::Running,
                cancellable: true,
                stage: Some(ClientTransitionStage::CheckEnvironment),
                message: None,
            })));
        },
    );
    cx.update(|cx| {
        let (steps, active) = model.read(cx).startup_steps();
        assert_eq!(active, Some(2));
        assert_eq!(steps[2], t("startup.step.check"));
        assert_eq!(steps[3], t("startup.step.workspace"));
    });
}

#[gpui_kit::test]
fn volume_changes_preview_immediately_and_save_once_after_idle(cx: &mut TestAppContext) {
    use cynapse_client::view_model::ClientIntent;
    use std::{cell::RefCell, rc::Rc, time::Duration};

    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (_window, model, _directory) = open_test_window_parts(cx, package, |_, _| {});
    let saves = Rc::new(RefCell::new(Vec::new()));
    let recorded = saves.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&model, move |_, event, _| {
            if let ClientIntent::SettingsChanged { settings, .. } = event {
                recorded.borrow_mut().push(settings.volume);
            }
        })
    });
    for volume in [21, 22, 23, 23] {
        cx.update(|cx| {
            model.update(cx, |model, cx| {
                model.set_volume(volume, cx);
                assert_eq!(model.settings().volume, volume);
            })
        });
        cx.run_until_parked();
    }
    assert!(saves.borrow().is_empty());
    cx.executor().advance_clock(Duration::from_secs(1));
    assert_eq!(*saves.borrow(), [23]);

    // A keyboard nudge has no release event.
    cx.update(|cx| model.update(cx, |model, cx| model.set_volume(24, cx)));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    assert_eq!(*saves.borrow(), [23, 24]);

    cx.update(|cx| {
        model.update(cx, |model, cx| {
            model.set_volume(25, cx);
            assert_eq!(model.take_pending_volume_settings().unwrap().volume, 25);
            assert!(model.take_pending_volume_settings().is_none());
        })
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    assert_eq!(*saves.borrow(), [23, 24]);
}

#[gpui_kit::test]
fn active_course_path_matches_a_non_canonical_target(cx: &mut TestAppContext) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (_window, view_model, _directory) = open_test_window_parts(cx, package.clone(), |_, _| {});

    cx.update(|cx| {
        assert!(view_model.read(cx).is_active_course_path(&package));
        view_model.update(cx, |view_model, cx| {
            view_model.start_course_import(
                "https://example.invalid/latest".into(),
                Some(package),
                cx,
            );
            assert_eq!(
                view_model.course_import_error().unwrap().message,
                t("course_import.error.active_course_overwrite")
            );
        });
    });
}

#[gpui_kit::test]
fn closing_url_course_conflict_returns_focus_to_main_view(cx: &mut TestAppContext) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (window, view_model, _directory) = open_test_window_parts(cx, package.clone(), |_, _| {});
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update(|cx| {
        view_model.update(cx, |view_model, cx| {
            view_model.set_course_store_dir(Some(package.parent().unwrap().to_path_buf()), cx);
        });
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        for _ in 0..20 {
            if window.find("open-course").focused() == Some(true) {
                break;
            }
            window.focus_next(cx);
            window.render_frame(cx);
        }
        assert_eq!(window.find("open-course").focused(), Some(true));
        window.click("open-url-course", cx);
        window.render_frame(cx);
        assert!(window.try_find("course-import-url-open").is_some());
    })
    .expect("main window");
    cx.simulate_input(window.into(), "https://example.invalid/multi-unit-demo.zip");
    cx.update_window(window.into(), |_, window, cx| {
        window.click("course-import-url-open", cx);
        window.render_frame(cx);
        window.click("course-import-cancel", cx);
        window.render_frame(cx);
        assert_eq!(window.find("main-view").focused(), Some(true));
        assert_eq!(window.find("open-course").focused(), Some(false));
    })
    .expect("main window");
}

#[gpui_kit::test]
fn course_conflict_dialog_offers_open_overwrite_and_cancel(cx: &mut TestAppContext) {
    let (window, view_model, directory) = open_test_window_parts(
        cx,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo"),
        |_, _| {},
    );
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window.into(), |_, window, cx| {
        let dialog_view = cx.new(|cx| {
            CourseImportDialogView::conflict(
                view_model,
                "http://example.com/course.zip".into(),
                directory.path().join("course"),
                window,
                cx,
            )
        });
        window.open_dialog(cx, move |dialog, _, _| dialog.child(dialog_view.clone()));
        window.render_frame(cx);
        assert!(window.find("course-import-open-existing").visible());
        assert!(window.find("course-import-overwrite").visible());
        assert!(window.find("course-import-cancel").visible());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn cancelling_the_reset_course_dialog_changes_nothing(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |_, state| {
        mark_unit_in_progress(state, "ch_units/first_unit");
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("reset-course").is_some());
        window.click("reset-course", cx);
        assert!(window.find("cancel").visible());
        window.click("cancel", cx);
        assert!(window.try_find("cancel").is_none());
        assert!(
            window.try_find("reset-course").is_some(),
            "cancelling reset must keep the course progress"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn main_view_shows_and_hides_the_reset_overlay(cx: &mut TestAppContext) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let (window, view_model, _directory) = open_test_window_parts(cx, package, |_, _| {});
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update(|cx| {
        view_model.update(cx, |view_model, cx| {
            view_model.set_reset_pending_for_test(true, cx);
        });
    });
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("busy-overlay").visible());
        assert!(window.find("busy-overlay-label").visible());
    })
    .expect("main window");

    cx.update(|cx| {
        view_model.update(cx, |view_model, cx| {
            view_model.set_reset_pending_for_test(false, cx);
        });
    });
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("busy-overlay").is_none());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn cancelling_the_close_course_dialog_keeps_the_course_open(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("close-course", cx);
        assert!(window.find("cancel").visible());
        window.click("cancel", cx);
        assert!(window.try_find("close-course").is_some());
        assert!(window.try_find("start-learning").is_some());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn cancelling_the_unit_reset_dialog_changes_nothing(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |_, state| {
        mark_unit_completed(state, "ch_units/first_unit");
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("unit-reset-ch_units/first_unit").is_some());
        window.click("unit-reset-ch_units/first_unit", cx);
        assert!(window.find("cancel").visible());
        window.click("cancel", cx);
        assert!(window.try_find("cancel").is_none());
        assert!(
            window.try_find("unit-reset-ch_units/first_unit").is_some(),
            "cancelling reset must leave the Unit untouched"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn unit_rows_keep_the_pre_ba9a270d_actions(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |_, state| {
        mark_unit_in_progress(state, "ch_units/first_unit");
        mark_unit_completed(state, "ch_units/second_unit");
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("unit-reset-ch_units/first_unit").is_some(),
            "a partial Unit keeps Reset"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/first_unit")
                .is_some(),
            "a selected partial Unit keeps Cancel"
        );
        assert!(
            window.try_find("unit-select-ch_units/first_unit").is_none(),
            "a selected partial Unit has no Start"
        );
        assert!(
            window.try_find("unit-reset-ch_units/second_unit").is_some(),
            "a completed Unit exposes Reset"
        );
        assert!(
            window
                .try_find("unit-select-ch_units/second_unit")
                .is_none(),
            "a completed Unit has no Start"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_none(),
            "a completed Unit has no Cancel"
        );
    })
    .expect("main window");
}

fn assert_toolbar_progress(
    cx: &mut TestAppContext,
    steps: Vec<StepView>,
    challenge_id: &'static str,
    step_id: &'static str,
) {
    let (window, _directory) = open_test_window_with(cx, |_, state| {
        set_unit_steps(state, "ch_units/first_unit", steps);
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(challenge_id).is_some(),
            "challenge progress {challenge_id} must appear in the toolbar"
        );
        assert!(
            window.try_find(step_id).is_some(),
            "step progress {step_id} must appear in the toolbar"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn toolbar_progress_renders_skipped_challenge_counts(cx: &mut TestAppContext) {
    assert_toolbar_progress(
        cx,
        vec![
            step("intro", false, ChallengeStatus::NotStarted),
            step("challenge", true, ChallengeStatus::Skipped),
        ],
        "unit-challenge-progress-1-0-1",
        "unit-step-progress-2-0-2",
    );
}

#[gpui_kit::test]
fn toolbar_progress_renders_passed_challenge_counts(cx: &mut TestAppContext) {
    assert_toolbar_progress(
        cx,
        vec![
            step("intro", false, ChallengeStatus::NotStarted),
            step("challenge", true, ChallengeStatus::Passed),
        ],
        "unit-challenge-progress-0-1-1",
        "unit-step-progress-2-0-2",
    );
}

#[gpui_kit::test]
fn toolbar_progress_renders_failed_challenge_counts(cx: &mut TestAppContext) {
    let mut failed = step("challenge", true, ChallengeStatus::Failed);
    failed.completed = false;
    assert_toolbar_progress(
        cx,
        vec![step("intro", false, ChallengeStatus::NotStarted), failed],
        "unit-challenge-progress-0-0-1",
        "unit-step-progress-1-1-2",
    );
}

#[gpui_kit::test]
fn warnings_render_as_a_compact_row(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, _| {
        model.set_warning(t("error.challenge_check_timeout"));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let warning = window.find("client-warning");
        assert!(warning.visible());
        assert!(
            warning.bounds().size.height < px(80.),
            "warning must not reserve a large empty block"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn next_action_hides_a_blocked_step_button(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        set_unit_steps(
            state,
            "ch_units/first_unit",
            vec![step("manual", false, ChallengeStatus::NotStarted)],
        );
        model.apply(ClientEvent::Playback(Box::new(
            PlaybackState::StepComplete {
                position: PlaybackPosition {
                    unit_path: "ch_units/first_unit".into(),
                    step_id: "manual".into(),
                    ..Default::default()
                },
                continuation: None,
                requires_human: false,
            },
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("next-step").is_none(),
            "a blocked advance must not leave a disabled button"
        );
        assert!(window.try_find("next-unit").is_none());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn stopping_learning_shows_only_continue_for_the_selected_unit(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        mark_unit_in_progress(state, "ch_units/first_unit");
        model.apply_course_state(state.clone());
        adapter_connected(model);
        model
            .start_learning(LearningMode::ByStep, "ch_units/first_unit")
            .unwrap();
        model.apply(ClientEvent::Playback(Box::new(
            PlaybackState::StepComplete {
                position: PlaybackPosition {
                    unit_path: "ch_units/first_unit".into(),
                    step_id: "shared_1".into(),
                    ..Default::default()
                },
                continuation: None,
                requires_human: false,
            },
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("next-step").is_some(),
            "an active by-step lesson offers the next Step"
        );
        window.click("stop-learning", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("next-step").is_none(),
            "Stop must not leave a stale next-Step action"
        );
        assert_eq!(
            window.find("start-learning").label(),
            Some(t("unit.continue").as_str())
        );
    })
    .expect("main window");
}

fn first_unit_completed_playback() -> PlaybackState {
    PlaybackState::UnitComplete {
        position: PlaybackPosition {
            unit_path: "ch_units/first_unit".into(),
            step_id: "shared_1".into(),
            ..Default::default()
        },
    }
}

#[gpui_kit::test]
fn no_selected_unit_hides_main_controls_and_current_unit_progress(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        if let Some(course) = state.active_course.as_mut() {
            course.current_unit_path = Some("ch_units/first_unit".into());
            course.current_step_id = Some("shared_1".into());
        }
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Idle)));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        for id in [
            "start-learning",
            "stop-learning",
            "next-step",
            "next-unit",
            "unit-step-progress-0-0-1",
        ] {
            assert!(
                window.try_find(id).is_none(),
                "{id} must be hidden while no Unit is selected"
            );
        }
        assert!(
            window
                .try_find("unit-unselect-ch_units/first_unit")
                .is_none(),
            "the resume cursor must not make an unselected Unit appear selected"
        );
        assert!(
            window.try_find("unit-select-ch_units/first_unit").is_some(),
            "the unselected Unit offers Start"
        );
        assert!(
            window
                .try_find("unit-progress-ch_units/first_unit")
                .is_some(),
            "the Course Tree keeps its per-Unit progress label"
        );
        assert!(
            window
                .try_find("unit-progress-ch_units/second_unit")
                .is_some(),
            "every Course Tree Unit keeps its progress label"
        );
    })
    .expect("main window");
}

fn last_unit_completed_playback() -> PlaybackState {
    PlaybackState::UnitComplete {
        position: PlaybackPosition {
            unit_path: "ch_units/second_unit".into(),
            step_id: "shared_1_ch_units_second_unit".into(),
            ..Default::default()
        },
    }
}

#[gpui_kit::test]
fn final_unit_completion_returns_to_idle_without_course_completion_controls(
    cx: &mut TestAppContext,
) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_units/second_unit");
        if let Some(course) = state.active_course.as_mut() {
            course.current_unit_path = None;
            course.current_step_id = None;
        }
        model.apply(ClientEvent::Playback(Box::new(
            last_unit_completed_playback(),
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        for id in [
            "start-learning",
            "stop-learning",
            "next-step",
            "next-unit",
            "next-unit-prompt",
            "unit-step-progress-0-0-1",
        ] {
            assert!(
                window.try_find(id).is_none(),
                "{id} must be hidden after the final Unit completes"
            );
        }
    })
    .expect("main window");
}

#[gpui_kit::test]
fn completed_next_unit_prompt_offers_reset_and_cancel(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_units/second_unit");
        model.apply(ClientEvent::Playback(Box::new(
            first_unit_completed_playback(),
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_some());
        assert_eq!(
            window.find("next-unit-reset").label(),
            Some(t("main.reset_after_learning").as_str())
        );
        assert_eq!(
            window.find("next-unit-cancel").label(),
            Some(t("common.cancel").as_str())
        );
        assert!(
            window.try_find("next-unit-continue").is_none(),
            "a completed Unit cannot be continued"
        );
        assert!(
            window.try_find("start-learning").is_none(),
            "the prompt appears while no Unit is selected"
        );
        assert!(window.try_find("stop-learning").is_none());
        window.click("next-unit-cancel", cx);
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_none());
        assert!(window.try_find("start-learning").is_none());
        assert!(
            window
                .try_find("unit-select-ch_units/second_unit")
                .is_none(),
            "a completed Unit exposes no Start"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_none(),
            "a completed Unit exposes no Cancel"
        );
        assert!(
            window.try_find("unit-reset-ch_units/second_unit").is_some(),
            "a completed Unit exposes Reset"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn partial_next_unit_prompt_offers_continue_and_reset(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        mark_unit_in_progress(state, "ch_units/second_unit");
        model.apply(ClientEvent::Playback(Box::new(
            first_unit_completed_playback(),
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_some());
        assert_eq!(
            window.find("next-unit-continue").label(),
            Some(t("main.continue_learning").as_str())
        );
        assert_eq!(
            window.find("next-unit-reset").label(),
            Some(t("main.reset_after_learning").as_str())
        );
        window.click("next-unit-continue", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_none());
        assert!(
            window.try_find("start-learning").is_some(),
            "continuing selects the partial Unit without a reset"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_some(),
            "continuing selects the partial next Unit"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn completed_cursor_next_unit_selects_the_following_unit(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_units/first_unit");
        state.selected_unit = find_unit(
            &state.active_course.as_ref().expect("course").tree,
            "ch_units/first_unit",
        )
        .cloned();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Ready {
            position: None,
        })));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("next-step").label(),
            Some(t("next.unit").as_str())
        );
        window.click("next-step", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_none(),
            "a completed cursor must not fail the next-Unit action"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_some(),
            "the following Unit becomes selected"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/first_unit")
                .is_none(),
            "the completed cursor loses selection"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn completed_cursor_next_unit_prompts_before_a_partial_target(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_units/first_unit");
        mark_unit_in_progress(state, "ch_units/second_unit");
        state.selected_unit = find_unit(
            &state.active_course.as_ref().expect("course").tree,
            "ch_units/first_unit",
        )
        .cloned();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Ready {
            position: None,
        })));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-step").is_none());
        assert!(window.try_find("next-unit-prompt").is_some());
        assert_eq!(
            window.find("next-unit-continue").label(),
            Some(t("main.continue_learning").as_str())
        );
        window.click("next-unit-continue", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_none());
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_some(),
            "continuing selects the partial target without resetting it"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn completed_cursor_prompt_cancel_stays_dismissed(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_units/first_unit");
        mark_unit_in_progress(state, "ch_units/second_unit");
        state.selected_unit = find_unit(
            &state.active_course.as_ref().expect("course").tree,
            "ch_units/first_unit",
        )
        .cloned();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Ready {
            position: None,
        })));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_some());
        window.click("next-unit-cancel", cx);
    })
    .expect("main window");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_none());
        assert!(
            window.try_find("unit-reset-ch_units/first_unit").is_some(),
            "cancelling keeps the completed cursor selected"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn selected_completed_cursor_does_not_auto_advance_from_the_playback_position(
    cx: &mut TestAppContext,
) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-node-demo");
    let (window, _directory) = open_test_window_with_package(cx, package, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_foundation/main");
        mark_unit_completed(state, "ch_configuration/main");
        state.selected_unit = find_unit(
            &state.active_course.as_ref().expect("course").tree,
            "ch_configuration/main",
        )
        .cloned();
        model.apply(ClientEvent::Playback(Box::new(
            PlaybackState::UnitComplete {
                position: PlaybackPosition {
                    unit_path: "ch_foundation/main".into(),
                    step_id: "foundation_1".into(),
                    ..Default::default()
                },
            },
        )));
    });
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_none());
        assert_eq!(
            window.find("next-step").label(),
            Some(t("next.unit").as_str()),
            "an explicit completed cursor exposes Next Unit"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_validation/main")
                .is_none(),
            "the playback position must not auto-select the next Unit"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn selected_completed_cursor_targets_the_following_unit_not_the_playback_position(
    cx: &mut TestAppContext,
) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-node-demo");
    let (window, _directory) = open_test_window_with_package(cx, package, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_foundation/main");
        mark_unit_in_progress(state, "ch_workspace_basics/main");
        mark_unit_completed(state, "ch_configuration/main");
        state.selected_unit = find_unit(
            &state.active_course.as_ref().expect("course").tree,
            "ch_configuration/main",
        )
        .cloned();
        model.apply(ClientEvent::Playback(Box::new(
            PlaybackState::UnitComplete {
                position: PlaybackPosition {
                    unit_path: "ch_foundation/main".into(),
                    step_id: "foundation_1".into(),
                    ..Default::default()
                },
            },
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_none());
        assert_eq!(
            window.find("next-step").label(),
            Some(t("next.unit").as_str())
        );
        window.click("next-step", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("client-warning").is_none());
        assert!(
            window
                .try_find("unit-unselect-ch_validation/main")
                .is_some(),
            "the target follows the selected completed Unit"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_workspace_basics/main")
                .is_none(),
            "the playback position must not choose the target"
        );
    })
    .expect("main window");
}

fn write_continuous_resume_course(root: &std::path::Path) -> PathBuf {
    std::fs::create_dir_all(root.join("steps")).unwrap();
    std::fs::write(
        root.join("manifest.json"),
        r#"{
            "id": "continuous_resume_demo",
            "title": "Continuous Resume Demo",
            "version": "1.0.0",
            "adapters": [],
            "workspace": "workspace",
            "children": [{
                "id": "ch",
                "title": "Chapter",
                "children": [
                    {
                        "id": "first",
                        "title": "First",
                        "steps": [{ "step_file": "steps/first.json" }]
                    },
                    {
                        "id": "second",
                        "title": "Second",
                        "steps": [{ "step_file": "steps/second.json" }]
                    }
                ]
            }]
        }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("steps/first.json"),
        r#"{ "step_id": "first", "title": "First", "actions": [{ "type": "Present" }] }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("steps/second.json"),
        r#"{
            "step_id": "second",
            "title": "Second",
            "requires_human": true,
            "actions": [{ "type": "Present" }]
        }"#,
    )
    .unwrap();
    root.to_path_buf()
}

#[gpui_kit::test]
fn continuous_learning_resumes_after_continuing_a_partial_next_unit(cx: &mut TestAppContext) {
    let package = tempfile::tempdir().expect("course temp dir");
    let package_path = write_continuous_resume_course(package.path());
    let (window, _directory) = open_test_window_with_package(cx, package_path, |model, state| {
        model.apply_course_state(state.clone());
        let selected = futures::executor::block_on(model.session().select_unit("ch/first"))
            .expect("select the first Unit");
        model.apply_course_state(selected);
        model
            .start_learning(LearningMode::Continuous, "ch/first")
            .expect("Continuous Learning starts");
        mark_unit_completed(state, "ch/first");
        mark_unit_in_progress(state, "ch/second");
        clear_unit_selection(state);
        if let Some(course) = state.active_course.as_mut() {
            course.current_unit_path = Some("ch/first".into());
        }
        model.apply(ClientEvent::Playback(Box::new(
            PlaybackState::UnitComplete {
                position: PlaybackPosition {
                    unit_path: "ch/first".into(),
                    step_id: "first".into(),
                    ..Default::default()
                },
            },
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("next-unit-prompt").is_some(),
            "Continuous Learning pauses before a studied next Unit"
        );
        assert!(
            window.try_find("stop-learning").is_none(),
            "the prompt appears after the paused mode stops"
        );
        window.click("next-unit-continue", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("next-unit-prompt").is_none());
        assert!(
            window.try_find("unit-step-progress-0-0-1").is_some(),
            "the selected next Unit's current progress is visible"
        );
        assert!(
            window.try_find("stop-learning").is_some(),
            "the learner's earlier Continuous Learning mode resumes"
        );
        assert!(
            window.try_find("client-warning").is_none(),
            "resuming the mode does not report a warning"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn failed_next_unit_reset_leaves_no_selection_and_reports_an_error(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        mark_unit_completed(state, "ch_units/second_unit");
        model.apply(ClientEvent::Playback(Box::new(
            first_unit_completed_playback(),
        )));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("next-unit-reset", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_some(),
            "a failed reset reports an error"
        );
        assert!(
            window.try_find("start-learning").is_none(),
            "selection happens only after a successful reset"
        );
        assert!(
            window
                .try_find("unit-select-ch_units/second_unit")
                .is_none(),
            "a completed Unit exposes no Start"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_none(),
            "a completed Unit exposes no Cancel"
        );
        assert!(
            window.try_find("unit-reset-ch_units/second_unit").is_some(),
            "a completed Unit exposes Reset"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn unit_completion_selects_a_not_started_next_unit(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_selection(state);
        model.apply(ClientEvent::Playback(Box::new(
            first_unit_completed_playback(),
        )));
    });
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("next-unit-prompt").is_none(),
            "a not-started Unit is selected directly"
        );
        assert_eq!(
            window.find("start-learning").label(),
            Some(t("unit.start").as_str()),
            "By-step Learning waits for the learner after selecting"
        );
        assert!(window.try_find("stop-learning").is_none());
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_some(),
            "the not-started next Unit is selected"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn stopped_learning_keeps_the_course_tree_actions(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, _| {
        model.stop_learning_local();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Paused {
            position: PlaybackPosition {
                unit_path: "ch_units/first_unit".into(),
                step_id: "first_step".into(),
                ..Default::default()
            },
            interruption: PlaybackInterruption {
                reason: PlaybackInterruptionReason::UserStop,
                exit_code: None,
                output: None,
                output_truncated: None,
            },
        })));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find("unit-unselect-ch_units/first_unit")
                .is_some(),
            "a stopped Unit must still expose its Cancel action"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn active_playback_after_a_failed_stop_keeps_the_stop_control(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, _| {
        // A failed Stop clears the local learning mode before Playback has stopped.
        model
            .start_learning_local(LearningMode::ByStep, "ch_units/first_unit")
            .unwrap();
        model.stop_learning_local();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Playing {
            action_id: "action".into(),
            action_label: "Action".into(),
            position: PlaybackPosition {
                unit_path: "ch_units/first_unit".into(),
                step_id: "first_step".into(),
                ..Default::default()
            },
        })));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("stop-learning").is_some(),
            "the still-running Playback keeps Stop available"
        );
        assert!(
            window.try_find("start-learning").is_none(),
            "Continue must not appear while Playback is running"
        );
    })
    .expect("main window");
}

fn adapter_connected(model: &mut ClientModel) {
    model.apply(ClientEvent::Connection(ConnectionState::Connected {
        adapter: "vscode".to_string(),
    }));
}

fn find_unit<'a>(nodes: &'a [CourseTreeNodeView], path: &str) -> Option<&'a UnitView> {
    for node in nodes {
        if let Some(unit) = node.unit.as_ref().filter(|unit| unit.path == path) {
            return Some(unit);
        }
        if let Some(unit) = find_unit(&node.children, path) {
            return Some(unit);
        }
    }
    None
}

fn clear_unit_adapters(state: &mut CourseSessionState, path: &str) {
    fn walk(nodes: &mut [CourseTreeNodeView], path: &str) {
        for node in nodes {
            if let Some(unit) = node.unit.as_mut().filter(|unit| unit.path == path) {
                unit.adapters.clear();
            }
            walk(&mut node.children, path);
        }
    }
    if let Some(course) = state.active_course.as_mut() {
        walk(&mut course.tree, path);
    }
    if let Some(unit) = state
        .selected_unit
        .as_mut()
        .filter(|unit| unit.path == path)
    {
        unit.adapters.clear();
    }
}

#[gpui_kit::test]
fn selected_adapter_free_unit_ignores_stale_playback_position(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        clear_unit_adapters(state, "ch_units/second_unit");
        state.selected_unit = find_unit(
            &state.active_course.as_ref().expect("course").tree,
            "ch_units/second_unit",
        )
        .cloned();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Paused {
            position: PlaybackPosition {
                unit_path: "ch_units/first_unit".into(),
                step_id: "first_step".into(),
                ..Default::default()
            },
            interruption: PlaybackInterruption {
                reason: PlaybackInterruptionReason::UserStop,
                exit_code: None,
                output: None,
                output_truncated: None,
            },
        })));
        model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_none(),
            "a selected Unit without required Adapters must ignore stale playback"
        );
        assert!(window.try_find("start-learning").is_some());
        window.click("start-learning", cx);
        window.click("step-start", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("stop-learning").is_some(),
            "the selected Unit without required Adapters can start"
        );
        assert!(window.try_find("client-warning").is_none());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn adapter_loss_stops_learning_and_restores_the_start_controls(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        model.apply_course_state(state.clone());
        adapter_connected(model);
        model
            .start_learning(LearningMode::ByStep, "ch_units/first_unit")
            .unwrap();
        model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("stop-learning").is_none(),
            "a forced Adapter stop must hide Stop"
        );
        assert!(
            window.try_find("start-learning").is_some(),
            "a forced Adapter stop must show Start again"
        );
        assert!(
            window.try_find("client-warning").is_some(),
            "the disconnected Adapter must stay visible in the shared warning area"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn playback_adapter_loss_stops_learning_and_shows_the_warning(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        model.apply_course_state(state.clone());
        adapter_connected(model);
        model
            .start_learning(LearningMode::ByStep, "ch_units/first_unit")
            .unwrap();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Paused {
            position: PlaybackPosition {
                unit_path: "ch_units/first_unit".into(),
                step_id: "first_step".into(),
                ..Default::default()
            },
            interruption: PlaybackInterruption {
                reason: PlaybackInterruptionReason::AdapterDisconnected,
                exit_code: None,
                output: None,
                output_truncated: None,
            },
        })));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("stop-learning").is_none(),
            "the Adapter-disconnect Playback must hide Stop"
        );
        assert!(
            window.try_find("start-learning").is_some(),
            "the Adapter-disconnect Playback must show Start again"
        );
        assert!(
            window.try_find("client-warning").is_some(),
            "the Adapter-disconnect Playback must show the warning"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn cancelling_selection_clears_the_adapter_warning(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_none(),
            "selecting a disconnected Unit must not show an error by itself"
        );
        window.click("start-learning", cx);
        window.click("step-start", cx);
        window.render_frame(cx);
        assert!(window.try_find("client-warning").is_some());
        window.click("unit-unselect-ch_units/first_unit", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_none(),
            "cancelling selection must clear the Adapter warning"
        );
        assert!(
            window.try_find("unit-select-ch_units/first_unit").is_some(),
            "the Unit must be selectable again"
        );
        window.click("unit-select-ch_units/first_unit", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_none(),
            "re-selecting the Unit must not revive the Adapter warning"
        );
        window.click("start-learning", cx);
        window.click("step-start", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_some(),
            "attempting Start after re-selection shows the connection error"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn reconnecting_does_not_auto_resume_learning(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        model.apply_course_state(state.clone());
        adapter_connected(model);
        model
            .start_learning(LearningMode::Continuous, "ch_units/first_unit")
            .unwrap();
        model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));
        adapter_connected(model);
        model.clear_warnings();
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("stop-learning").is_none());
        assert!(window.try_find("start-learning").is_some());
        assert!(
            window.try_find("client-warning").is_none(),
            "a reconnect clears the persistent Adapter warning"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn starting_while_disconnected_shows_the_connection_warning(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_none(),
            "selection alone must not show an Adapter error"
        );
        window.click("start-learning", cx);
        window.click("step-start", cx);
        window.render_frame(cx);
        assert!(window.try_find("client-warning").is_some());
        assert!(window.try_find("stop-learning").is_none());
        assert!(window.try_find("start-learning").is_some());

        window.click("start-learning", cx);
        window.click("step-start", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("client-warning").is_some(),
            "a second Start must report the same connection problem"
        );
        assert!(window.try_find("stop-learning").is_none());
    })
    .expect("main window");
}

#[gpui_kit::test]
fn retry_is_hidden_without_a_resumable_position(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |model, state| {
        model.apply_course_state(state.clone());
        adapter_connected(model);
        model
            .start_learning(LearningMode::ByStep, "ch_units/first_unit")
            .unwrap();
        model.apply(ClientEvent::Playback(Box::new(PlaybackState::Error {
            code: "persistence_failed".to_string(),
            message: "database is locked".to_string(),
            position: None,
            interruption: None,
        })));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("retry").is_none(),
            "Retry needs a resumable position"
        );
        assert!(
            window.try_find("stop-learning").is_some(),
            "a runtime failure keeps the mode active"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn cancelling_the_unit_selection_restores_the_select_action(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |_, state| {
        mark_unit_in_progress(state, "ch_units/first_unit");
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("unit-unselect-ch_units/first_unit", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("unit-select-ch_units/first_unit").is_some(),
            "cancelling selection must leave the Unit selectable again"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn cancelling_a_not_started_unit_hides_main_start(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("unit-unselect-ch_units/first_unit", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("unit-select-ch_units/first_unit").is_some(),
            "the row offers Start after Cancel"
        );
        assert!(
            window.try_find("start-learning").is_none(),
            "the main Start control stays hidden with no selected Unit"
        );
    })
    .expect("main window");
}

#[gpui_kit::test]
fn selecting_another_unit_replaces_the_selected_tree_row(cx: &mut TestAppContext) {
    let (window, _directory) = open_test_window_with(cx, |_, state| {
        mark_unit_in_progress(state, "ch_units/first_unit");
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find("unit-unselect-ch_units/first_unit")
                .is_some()
        );
        window.click("unit-select-ch_units/second_unit", cx);
    })
    .expect("main window");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("unit-select-ch_units/first_unit").is_some(),
            "the previously selected Unit must lose the selected action"
        );
        assert!(
            window
                .try_find("unit-unselect-ch_units/second_unit")
                .is_some(),
            "the newly selected Unit owns the selected action"
        );
    })
    .expect("main window");
}

fn open_learner_wait_window(
    cx: &mut TestAppContext,
    prepare: impl FnOnce(&mut ClientModel, &mut CourseSessionState),
) -> (
    gpui_kit::WindowHandle<Root>,
    Entity<ClientViewModel>,
    tempfile::TempDir,
) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    open_test_window_parts(cx, package, prepare)
}

fn open_wait_state_window(
    cx: &mut TestAppContext,
    playback: PlaybackState,
    requires_human: bool,
) -> (
    gpui_kit::WindowHandle<Root>,
    Entity<ClientViewModel>,
    tempfile::TempDir,
) {
    open_wait_state_window_after(cx, playback, requires_human, |_| {})
}

fn open_wait_state_window_after(
    cx: &mut TestAppContext,
    playback: PlaybackState,
    requires_human: bool,
    after: impl FnOnce(&mut ClientModel),
) -> (
    gpui_kit::WindowHandle<Root>,
    Entity<ClientViewModel>,
    tempfile::TempDir,
) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let step_id = if requires_human { "human" } else { "manual" };
    open_test_window_parts_after(
        cx,
        package,
        |_, state| {
            let mut first = step(step_id, false, ChallengeStatus::NotStarted);
            first.requires_human = requires_human;
            set_unit_steps(
                state,
                "ch_units/first_unit",
                vec![first, step("after", false, ChallengeStatus::NotStarted)],
            );
            state.selected_unit = find_unit(
                &state.active_course.as_ref().expect("course").tree,
                "ch_units/first_unit",
            )
            .cloned();
        },
        |model| {
            adapter_connected(model);
            model.apply(ClientEvent::Playback(Box::new(playback)));
            model.apply(ClientEvent::Learning(LearningView {
                mode: Some(LearningMode::ByStep),
                phase: LearningPhase::WaitingForStep,
                unit_path: Some("ch_units/first_unit".into()),
                error: None,
            }));
            after(model);
        },
    )
}

fn open_unit_boundary_window(
    cx: &mut TestAppContext,
    playback: PlaybackState,
    final_unit: bool,
) -> (
    gpui_kit::WindowHandle<Root>,
    Entity<ClientViewModel>,
    tempfile::TempDir,
) {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    open_test_window_parts_after(
        cx,
        package,
        |_, state| {
            clear_unit_selection(state);
            mark_unit_completed(state, "ch_units/second_unit");
            if final_unit && let Some(course) = state.active_course.as_mut() {
                course.current_unit_path = None;
                course.current_step_id = None;
            }
        },
        |model| {
            model.apply(ClientEvent::Playback(Box::new(playback)));
        },
    )
}

fn manual_step_position() -> PlaybackPosition {
    PlaybackPosition {
        unit_path: "ch_units/first_unit".into(),
        step_id: "manual".into(),
        ..Default::default()
    }
}

fn manual_step_complete() -> PlaybackState {
    PlaybackState::StepComplete {
        position: manual_step_position(),
        continuation: None,
        requires_human: false,
    }
}

fn human_step_complete() -> PlaybackState {
    PlaybackState::StepComplete {
        position: PlaybackPosition {
            unit_path: "ch_units/first_unit".into(),
            step_id: "human".into(),
            ..Default::default()
        },
        continuation: None,
        requires_human: true,
    }
}

fn paused_state(reason: PlaybackInterruptionReason) -> PlaybackState {
    PlaybackState::Paused {
        position: manual_step_position(),
        interruption: PlaybackInterruption {
            reason,
            exit_code: None,
            output: None,
            output_truncated: None,
        },
    }
}

#[gpui_kit::test]
fn learner_wait_covers_manual_next(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) =
        open_wait_state_window(cx, manual_step_complete(), false);
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_some());
        });
    });
}

#[gpui_kit::test]
fn learner_wait_covers_retry(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) = open_wait_state_window(
        cx,
        PlaybackState::Error {
            code: "E_TEST".into(),
            message: "boom".into(),
            position: Some(manual_step_position()),
            interruption: None,
        },
        false,
    );
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_some());
        });
    });
}

#[gpui_kit::test]
fn learner_wait_covers_next_unit_prompt(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) =
        open_unit_boundary_window(cx, first_unit_completed_playback(), false);
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_some());
        });
    });
}

#[gpui_kit::test]
fn cancelled_next_unit_prompt_is_not_a_learner_wait(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) =
        open_unit_boundary_window(cx, first_unit_completed_playback(), false);
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, cx| {
            assert!(view_model.awaiting_learner_action());
            view_model.cancel_next_unit(
                view_model.course_identity(),
                Some("ch_units/first_unit".into()),
                "ch_units/second_unit".into(),
                cx,
            );
            assert!(!view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_none());
        });
    });
}

#[gpui_kit::test]
fn final_unit_completion_is_not_a_learner_wait(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) =
        open_unit_boundary_window(cx, last_unit_completed_playback(), true);
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(!view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_none());
        });
    });
}

#[gpui_kit::test]
fn learner_wait_ignores_start_controls(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) = open_learner_wait_window(cx, |_, _| {});
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(!view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_none());
        });
    });
}

#[gpui_kit::test]
fn learner_wait_covers_passive_pause(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) =
        open_wait_state_window(cx, paused_state(PlaybackInterruptionReason::Timeout), false);
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_some());
        });
    });
}

#[gpui_kit::test]
fn learner_wait_ignores_user_stop(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) = open_wait_state_window(
        cx,
        paused_state(PlaybackInterruptionReason::UserStop),
        false,
    );
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(!view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_none());
        });
    });
}

#[gpui_kit::test]
fn stopping_a_human_step_clears_the_wait(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) = open_wait_state_window(cx, human_step_complete(), true);
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, cx| {
            view_model.stop_learning(cx);
            assert!(
                !view_model.awaiting_learner_action(),
                "Stop must clear the human wait"
            );
            assert!(view_model.learner_wait_key().is_none());
        });
    });
}

#[gpui_kit::test]
fn an_ended_session_does_not_wait_for_a_lost_adapter(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) =
        open_wait_state_window_after(cx, manual_step_complete(), false, |model| {
            model.stop_learning_local();
            // The heartbeat drops an idle Adapter about 60s after the last
            // message. The leftover StepComplete used to read as a learner
            // wait, so the hidden window was pulled back out of the tray.
            model.apply(ClientEvent::Connection(ConnectionState::WaitingForAdapter));
        });
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(
                !view_model.awaiting_learner_action(),
                "a stopped session must not wait for a lost Adapter"
            );
            assert!(view_model.learner_wait_key().is_none());
        });
    });
}

#[gpui_kit::test]
fn learner_wait_ignores_active_playback(cx: &mut TestAppContext) {
    let (_window, view_model, _directory) = open_wait_state_window(
        cx,
        PlaybackState::Playing {
            action_id: "action".into(),
            action_label: "Action".into(),
            position: manual_step_position(),
        },
        false,
    );
    cx.run_until_parked();
    cx.update(|cx| {
        view_model.update(cx, |view_model, _| {
            assert!(!view_model.awaiting_learner_action());
            assert!(view_model.learner_wait_key().is_none());
        });
    });
}
