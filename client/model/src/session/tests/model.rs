//! ClientModel state, startup, and selection tests.

use super::support::*;
use super::*;
use cynapse_client_core::DiscoveryFile;
use cynapse_client_i18n::t;
use std::fs;
use std::time::Duration;
use tokio::time::timeout;

#[test]
fn connection_wait_fails_fast_on_adapter_error() {
    assert_eq!(
        connection_wait_result(&ConnectionState::Error {
            code: "invalid_origin".to_string(),
            message: "invalid origin".to_string(),
        }),
        Some(Err("invalid origin".to_string()))
    );
    assert_eq!(
        connection_wait_result(&ConnectionState::Connected {
            adapter: "vscode".to_string(),
        }),
        Some(Ok(()))
    );
    assert_eq!(
        connection_wait_result(&ConnectionState::WaitingForAdapter),
        None
    );
}

#[test]
fn local_learning_start_sets_the_view_without_a_session_transition() {
    let directory = tempfile::tempdir().unwrap();
    let (mut model, _receiver) = crate::ClientModel::new_at(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
    )
    .unwrap();

    model
        .start_learning_local(crate::LearningMode::ByStep, "ch/unit")
        .unwrap();

    assert_eq!(
        model.state().learning().mode,
        Some(crate::LearningMode::ByStep)
    );
    assert_eq!(
        model.state().learning().phase,
        cynapse_client_core::LearningPhase::Running
    );
    assert!(
        model
            .start_learning_local(crate::LearningMode::ByStep, "ch/unit")
            .is_err()
    );
}

#[test]
fn a_connected_start_does_not_republish_a_connection() {
    let connected = ConnectionState::Connected {
        adapter: "vscode".to_string(),
    };
    assert!(!should_republish_start_connection(
        Some(ClientTransitionKind::Starting),
        &connected,
    ));
    assert!(should_republish_start_connection(
        Some(ClientTransitionKind::Starting),
        &ConnectionState::WaitingForAdapter,
    ));
    assert!(!should_republish_start_connection(
        Some(ClientTransitionKind::Opening),
        &ConnectionState::WaitingForAdapter,
    ));
}

#[test]
fn starts_the_protocol_server_before_a_course_is_open() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();

    service.start_server_at_startup().unwrap();

    let runtime: DiscoveryFile =
        serde_json::from_str(&fs::read_to_string(directory.path().join("runtime.json")).unwrap())
            .unwrap();
    assert_eq!(runtime.workspace, "");
    assert!(runtime.port > 0);
}

#[test]
fn startup_course_opens_before_the_initial_model_state() {
    let directory = tempfile::tempdir().unwrap();
    let package =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/independent-units");
    let (model, _receiver) = crate::ClientModel::new_at_with_startup_course(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
        Some(package),
    )
    .unwrap();

    assert!(model.course_title().is_some());
    assert!(model.state().warnings().is_empty());
}

#[test]
fn failed_startup_course_keeps_the_restored_course_and_warns() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let package =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/independent-units");
    let mut session = cynapse_client_core::CourseSession::new(&db_path).unwrap();
    session.open_course(&package).unwrap();
    drop(session);

    let (model, _receiver) = crate::ClientModel::new_at_with_startup_course(
        crate::ClientSettings::default(),
        db_path,
        Some(std::path::PathBuf::from("missing-startup-course")),
    )
    .unwrap();

    assert!(model.course_title().is_some());
    let warning_prefix = t("warning.startup_course_open_failed");
    let warning_prefix = warning_prefix.split("%{").next().unwrap_or("");
    assert!(
        model
            .state()
            .warnings()
            .iter()
            .any(|warning| warning.text().contains(warning_prefix))
    );
}

#[test]
fn a_missing_active_workspace_does_not_fall_back_to_the_package_root() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);

    futures::executor::block_on(service.open_course(package.clone())).unwrap();
    fs::remove_dir_all(package.join("workspace")).unwrap();
    service.start_server_at_startup().unwrap();

    let runtime: DiscoveryFile =
        serde_json::from_str(&fs::read_to_string(directory.path().join("runtime.json")).unwrap())
            .unwrap();
    assert_eq!(runtime.workspace, "");
}

#[test]
fn opens_a_course_and_selects_a_unit() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/multi-unit-demo");

    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    assert!(state.active_course.is_some());
    let unit = state.selected_unit.expect("startup unit");
    let selected = futures::executor::block_on(service.select_unit(&unit.path)).unwrap();
    assert_eq!(
        selected
            .selected_unit
            .as_ref()
            .map(|unit| unit.path.as_str()),
        Some(unit.path.as_str())
    );

    let second = "ch_units/second_unit";
    let selected = futures::executor::block_on(service.select_unit(second)).unwrap();
    assert_eq!(
        selected
            .selected_unit
            .as_ref()
            .map(|unit| unit.path.as_str()),
        Some(second)
    );

    let cleared = futures::executor::block_on(service.clear_selection()).unwrap();
    assert!(cleared.selected_unit.is_none());
}

#[test]
fn a_jupyterlab_course_reports_the_jupyterlab_adapter() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_adapter_course(directory.path(), "jupyterlab");
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    futures::executor::block_on(service.open_course(package)).unwrap();
    let adapter = test_runtime.spawn(fake_adapter_hello_ack(
        directory.path().join("runtime.json"),
    ));

    let reported = test_runtime.block_on(async {
        timeout(Duration::from_secs(5), adapter)
            .await
            .expect("timed out waiting for hello_ack")
            .unwrap()
    });

    assert_eq!(reported, "jupyterlab");
}
