//! Courses (or Units) that need no external Adapter run on the local adapter.
//!
//! Spec §4.2: a node whose effective `adapters` is empty may only contain
//! `Present` and `Wait`, and must still play when no external App is
//! connected. The local adapter is what answers `Present` in that case, so
//! these tests connect no socket of their own: they fail if the server is
//! started with an external Adapter identity instead.

use super::support::*;
use super::*;

#[test]
fn a_course_without_adapters_plays_without_an_external_adapter() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_adapterless_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;

    test_runtime.block_on(async {
        // Nothing connects from this test: reaching Connected means the local
        // adapter was spawned and authenticated itself.
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        let _ = service.stop_learning().await;
    });

    assert!(
        service.server_adapter().is_none(),
        "a local run must not report an external Adapter identity"
    );
}

/// The Local branch is not only for whole Courses without Adapters: a local-only
/// Unit inside an adapter Course is admissible (spec §4.2), and opening the
/// Course must serve its startup Unit from the built-in local adapter instead
/// of waiting for the external App the rest of the Course names.
#[test]
fn a_local_only_startup_unit_plays_locally_inside_an_adapter_course() {
    let test_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let package = write_local_only_unit_course(directory.path());
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    let state = futures::executor::block_on(service.open_course(package)).unwrap();
    let unit_path = state.selected_unit.expect("startup unit").path;
    assert_eq!(
        unit_path, "ch/solo",
        "the local-only Unit is the startup Unit"
    );

    test_runtime.block_on(async {
        // Reaching Connected without this test connecting a socket means the
        // server targeted the local adapter for the local-only startup Unit,
        // not the VS Code the Course manifest names.
        wait_for_connection(&mut receiver, true).await;
        service
            .begin_learning(LearningMode::ByStep, &unit_path)
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        let _ = service.stop_learning().await;
    });
}
