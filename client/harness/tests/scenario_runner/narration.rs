use super::*;

#[tokio::test]
async fn scenario_runner_drives_action_narration_events_without_real_media_devices() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("narration-demo"),
        unit_path: Some(format!("{}/{}", "ch_narration", "sec_narration")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript::acknowledge_all(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    for (step_id, action_index, phase) in [
        ("create_intro", 0, "before"),
        ("write_edit", 2, "during"),
        ("run_check", 0, "during"),
        ("create_debrief", 0, "after"),
    ] {
        assert!(
            report.events.iter().any(|event| {
                event.kind == ScenarioEventKind::NarrationCompleted
                    && event.detail["step_id"] == serde_json::json!(step_id)
                    && event.detail["clip"]["phase"] == serde_json::json!(phase)
                    && event.detail["clip"]["action_index"] == serde_json::json!(action_index)
                    && event.detail["clip"]["clip_index"] == serde_json::json!(0)
            }),
            "missing narration event for {step_id} {phase}"
        );
    }
}

#[tokio::test]
async fn narration_timeout_is_scriptable_and_retained() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("narration-demo"),
        unit_path: Some(format!("{}/{}", "ch_narration", "sec_narration")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            narration: NarrationReply::Timeout,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_millis(50),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::TimedOut);
    assert!(report.artifacts_retained);
    assert!(
        report
            .events
            .iter()
            .any(|event| event.kind == ScenarioEventKind::AdapterTimeout)
    );
}

#[tokio::test]
async fn narration_failure_is_reported_and_retains_the_unit() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("narration-demo"),
        unit_path: Some(format!("{}/{}", "ch_narration", "sec_narration")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript {
            narration: NarrationReply::Fail,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::PlaybackError);
    assert!(report.events.iter().any(|event| {
        event.kind == ScenarioEventKind::PlaybackError
            && event.detail["code"] == serde_json::json!("narration_failed")
            && event.detail["message"]
                .as_str()
                .is_some_and(|message| message.contains("scripted media failure"))
    }));
    let course = report.course_state.active_course.unwrap();
    assert_eq!(
        course.current_unit_path.as_deref(),
        Some("ch_narration/sec_narration")
    );
    assert_eq!(course.current_step_id.as_deref(), Some("create_intro"));
}

#[tokio::test]
async fn action_before_narration_completes_before_the_action_is_received() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("action-before-narration-demo"),
        unit_path: Some(format!("{}/{}", "before", "demo")),
        mode: ScenarioMode::StopAfterStep,
        adapter_script: AdapterScript::default(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    let narration = report
        .events
        .iter()
        .find(|event| {
            event.kind == ScenarioEventKind::NarrationCompleted
                && event.detail["clip"]["phase"] == serde_json::json!("before")
                && event.detail["clip"]["action_index"] == serde_json::json!(0)
                && event.detail["clip"]["clip_index"] == serde_json::json!(0)
        })
        .expect("Before Clip completion");
    let action = report
        .events
        .iter()
        .find(|event| event.kind == ScenarioEventKind::ActionReceived)
        .expect("dispatched Action");
    assert!(narration.sequence < action.sequence);
    assert_eq!(action.detail["payload"]["type"], "CreateFile");
    assert!(action.detail["payload"].get("narration").is_none());
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path.ends_with("before-narration.txt"))
    );
}

#[tokio::test]
async fn action_before_narration_failure_and_retry_never_units_the_step() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("action-before-narration-demo"),
        unit_path: Some(format!("{}/{}", "before", "demo")),
        mode: ScenarioMode::StopAfterStep,
        adapter_script: AdapterScript {
            narration: NarrationReply::FailOnce,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    let actions = report
        .events
        .iter()
        .filter(|event| event.kind == ScenarioEventKind::ActionReceived)
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    let failure = report
        .events
        .iter()
        .find(|event| {
            event.kind == ScenarioEventKind::PlaybackError
                && event.detail["code"] == serde_json::json!("narration_failed")
        })
        .expect("retryable media failure");
    assert_eq!(failure.detail["position"]["unit_path"], "before/demo");
    assert_eq!(failure.detail["position"]["step_id"], "create_notice");
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path.ends_with("before-narration.txt"))
    );
}

#[tokio::test]
async fn action_narration_phase_combinations_complete_the_step() {
    for unit_id in ["before", "during", "after", "before_during", "before_after"] {
        let artifacts = tempfile::tempdir().unwrap();
        let report = run_scenario(ScenarioSpec {
            course_path: fixture("phase-timeline-demo"),
            unit_path: Some(format!("{}/{}", "phases", unit_id)),
            mode: ScenarioMode::StopAfterStep,
            adapter_script: AdapterScript::default(),
            adapter_script_path: None,
            artifact_root: artifacts.path().to_path_buf(),
            response_timeout: Duration::from_secs(1),
        })
        .await
        .unwrap();

        assert_eq!(report.status, ScenarioStatus::Completed, "{unit_id}");
        assert_eq!(
            report
                .events
                .iter()
                .filter(|event| event.kind == ScenarioEventKind::ActionReceived)
                .count(),
            2,
            "{unit_id}"
        );
        assert!(
            report
                .events
                .iter()
                .any(|event| event.kind == ScenarioEventKind::NarrationCompleted),
            "{unit_id}"
        );
    }
}

#[tokio::test]
async fn caption_only_narration_completes_through_the_scenario_runner() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("phase-timeline-demo"),
        unit_path: Some("phases/caption_only".to_string()),
        mode: ScenarioMode::StopAfterStep,
        adapter_script: AdapterScript::default(),
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    assert!(
        report
            .events
            .iter()
            .any(|event| event.kind == ScenarioEventKind::NarrationCompleted)
    );
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path.ends_with("caption-only.txt"))
    );
}

#[tokio::test]
async fn during_narration_supports_both_media_and_action_completion_orders() {
    for (narration, narration_first) in [
        (NarrationReply::Complete, true),
        (NarrationReply::EndAfterActionAck, false),
    ] {
        let artifacts = tempfile::tempdir().unwrap();
        let report = run_scenario(ScenarioSpec {
            course_path: fixture("phase-timeline-demo"),
            unit_path: Some(format!("{}/{}", "phases", "during")),
            mode: ScenarioMode::StopAfterStep,
            adapter_script: AdapterScript {
                narration,
                ..AdapterScript::default()
            },
            adapter_script_path: None,
            artifact_root: artifacts.path().to_path_buf(),
            response_timeout: Duration::from_secs(1),
        })
        .await
        .unwrap();
        let narration_sequence = report
            .events
            .iter()
            .find(|event| event.kind == ScenarioEventKind::NarrationCompleted)
            .expect("narration completion")
            .sequence;
        let action_sequence = report
            .events
            .iter()
            .find(|event| event.kind == ScenarioEventKind::ActionReceived)
            .expect("Action")
            .sequence;

        assert_eq!(
            narration_sequence < action_sequence,
            narration_first,
            "{narration:?}"
        );
        assert_eq!(report.status, ScenarioStatus::Completed);
    }
}

#[tokio::test]
async fn continuous_learning_waits_for_action_narration_before_the_next_step() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("phase-timeline-demo"),
        unit_path: Some(format!("{}/{}", "phases", "mode_boundaries")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript {
            narration: NarrationReply::EndAfterActionAck,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::WaitingForHuman);
    let narration_sequence = report
        .events
        .iter()
        .find(|event| event.kind == ScenarioEventKind::NarrationCompleted)
        .expect("narration completion")
        .sequence;
    let next_step_sequence = report
        .events
        .iter()
        .find(|event| {
            event.kind == ScenarioEventKind::ActionReceived
                && event.detail["payload"]["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("tail-second.txt"))
        })
        .expect("next Step Action")
        .sequence;
    assert!(
        narration_sequence < next_step_sequence,
        "Continuous Learning must wait for the Action narration tail"
    );
}

#[tokio::test]
async fn long_during_narration_finishes_before_the_next_step() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("narration-demo"),
        unit_path: Some(format!("{}/{}", "ch_narration", "sec_long_narration")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript {
            narration: NarrationReply::Complete,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let narration_sequence = report
        .events
        .iter()
        .find(|event| {
            event.kind == ScenarioEventKind::NarrationCompleted
                && event.detail["clip"]["phase"] == serde_json::json!("during")
                && event.detail["clip"]["action_index"] == serde_json::json!(2)
                && event.detail["clip"]["clip_index"] == serde_json::json!(0)
        })
        .expect("long During narration completion")
        .sequence;
    let second_step_sequence = report
        .events
        .iter()
        .find(|event| {
            event.kind == ScenarioEventKind::ActionReceived
                && event.detail["payload"]["target"]["line"] == serde_json::json!(2)
        })
        .expect("second long-narration Step")
        .sequence;
    assert!(narration_sequence < second_step_sequence);
}

#[tokio::test]
async fn continuous_learning_waits_for_narration_before_human_work() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("phase-timeline-demo"),
        unit_path: Some(format!("{}/{}", "phases", "human_tail")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript {
            narration: NarrationReply::EndAfterActionAck,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::WaitingForHuman);
    let narration_sequence = report
        .events
        .iter()
        .find(|event| event.kind == ScenarioEventKind::NarrationCompleted)
        .expect("narration completion")
        .sequence;
    let human_sequence = report
        .events
        .iter()
        .find(|event| event.kind == ScenarioEventKind::HumanStepReached)
        .expect("human boundary")
        .sequence;
    assert!(
        narration_sequence < human_sequence,
        "human work must wait for the Action narration tail"
    );
}

#[tokio::test]
async fn action_narration_timeout_retains_the_current_step() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("phase-timeline-demo"),
        unit_path: Some(format!("{}/{}", "phases", "mode_boundaries")),
        mode: ScenarioMode::ByStep,
        adapter_script: AdapterScript {
            narration: NarrationReply::Timeout,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_millis(50),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::TimedOut);
    assert!(report.artifacts_retained);
    let course = report.course_state.active_course.unwrap();
    assert_eq!(
        course.current_unit_path.as_deref(),
        Some("phases/mode_boundaries")
    );
    assert_eq!(course.current_step_id.as_deref(), Some("tail_first"));
}

#[tokio::test]
async fn action_narration_disconnect_retains_the_current_step() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("phase-timeline-demo"),
        unit_path: Some(format!("{}/{}", "phases", "mode_boundaries")),
        mode: ScenarioMode::Continuous,
        adapter_script: AdapterScript {
            narration: NarrationReply::Disconnect,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::AdapterDisconnected);
    let course = report.course_state.active_course.unwrap();
    assert_eq!(
        course.current_unit_path.as_deref(),
        Some("phases/mode_boundaries")
    );
    assert_eq!(course.current_step_id.as_deref(), Some("tail_first"));
}

#[tokio::test]
async fn action_narration_retry_restarts_the_step_and_replays_the_clip() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("phase-timeline-demo"),
        unit_path: Some(format!("{}/{}", "phases", "retry_from_first")),
        mode: ScenarioMode::StopAfterStep,
        adapter_script: AdapterScript {
            narration: NarrationReply::FailOnce,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Completed);
    let first_action_dispatches = report
        .events
        .iter()
        .filter(|event| {
            event.kind == ScenarioEventKind::ActionReceived
                && event.detail["payload"]["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("retry-first.txt"))
        })
        .count();
    assert_eq!(
        first_action_dispatches, 2,
        "retry must restart the Step from its first Action"
    );
    let narration_starts = report
        .events
        .iter()
        .filter(|event| {
            event.kind == ScenarioEventKind::PlaybackState
                && event.detail["state"] == serde_json::json!("narrating")
                && event.detail["position"]["step_id"] == serde_json::json!("retry_second_action")
                && event.detail["clip"]["phase"] == serde_json::json!("during")
                && event.detail["clip"]["action_index"] == serde_json::json!(1)
                && event.detail["clip"]["clip_index"] == serde_json::json!(0)
        })
        .count();
    assert_eq!(
        narration_starts, 2,
        "retry must replay the failed Action narration"
    );
}

#[tokio::test]
async fn stopping_at_a_narration_tail_retains_the_unit_progress() {
    let artifacts = tempfile::tempdir().unwrap();
    let report = run_scenario(ScenarioSpec {
        course_path: fixture("phase-timeline-demo"),
        unit_path: Some(format!("{}/{}", "phases", "mode_boundaries")),
        mode: ScenarioMode::StopAfterStep,
        adapter_script: AdapterScript {
            narration: NarrationReply::EndAfterActionAck,
            ..AdapterScript::default()
        },
        adapter_script_path: None,
        artifact_root: artifacts.path().to_path_buf(),
        response_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();

    assert_eq!(report.status, ScenarioStatus::Stopped);
    assert!(
        report
            .workspace_files
            .iter()
            .any(|path| path.ends_with("tail-first.txt"))
    );
    assert!(
        !report
            .workspace_files
            .iter()
            .any(|path| path.ends_with("tail-second.txt"))
    );
    let course = report.course_state.active_course.unwrap();
    assert_eq!(course.current_unit_path, None);
    assert_eq!(
        unit_by_path(&course.tree, "phases/mode_boundaries").status,
        cynapse_client_core::UnitStatus::InProgress
    );
}
