//! JupyterLab lifecycle and Adapter switching tests.

use super::support::*;
use super::*;
use cynapse_client_core::DiscoveryFile;
use std::fs;
use std::time::Duration;
use tokio::time::timeout;

/// Generous enough for a loaded CI runner; a real hang still fails.
const TEST_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[test]
fn a_jupyterlab_start_reuses_the_browser_tab_until_exit() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (service, mut receiver) =
        jupyterlab_service(directory.path(), host.clone(), 0, TEST_HANDSHAKE_TIMEOUT);
    assert_eq!(
        host.state
            .cleanups
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Client startup must clean a stale JupyterLab server"
    );
    let unit = "ch/main";
    let asset = runtime.spawn(fake_adapter_reconnecting(
        directory.path().join("runtime.json"),
    ));

    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, unit)).unwrap();

    assert_eq!(host.state.starts.lock().unwrap().len(), 1);
    assert!(host.state.running.load(std::sync::atomic::Ordering::SeqCst));
    let mut opened = 0;
    while let Ok(event) = receiver.try_recv() {
        if let ClientEvent::OpenUrl(url) = event {
            assert_eq!(url, "http://127.0.0.1:18890/lab?token=launch");
            opened += 1;
        }
    }
    assert!(opened >= 1, "starting JupyterLab must open the browser");

    futures::executor::block_on(service.stop_learning()).unwrap();
    assert_eq!(
        host.state.stops.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "Stop must leave the JupyterLab server alive"
    );
    assert!(host.state.running.load(std::sync::atomic::Ordering::SeqCst));

    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, unit)).unwrap();
    assert_eq!(
        host.state.starts.lock().unwrap().len(),
        1,
        "the same Active Course must reuse its server"
    );
    let mut reopened = 0;
    while let Ok(event) = receiver.try_recv() {
        if matches!(event, ClientEvent::OpenUrl(_)) {
            reopened += 1;
        }
    }
    assert_eq!(
        reopened, 0,
        "a reused connected server must not open a new browser tab"
    );

    asset.abort();
    futures::executor::block_on(service.cancel_for_exit()).unwrap();
    assert_eq!(
        host.state.stops.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "graceful exit must stop the JupyterLab server"
    );
    assert!(!host.state.running.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn a_reused_jupyterlab_restarts_a_missing_protocol_server() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (service, _receiver) =
        jupyterlab_service(directory.path(), host.clone(), 0, TEST_HANDSHAKE_TIMEOUT);
    let unit = "ch/main";
    let adapter = runtime.spawn(fake_adapter_reconnecting(
        directory.path().join("runtime.json"),
    ));

    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, unit)).unwrap();
    futures::executor::block_on(service.stop_server());
    assert!(host.state.running.load(std::sync::atomic::Ordering::SeqCst));

    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, unit)).unwrap();
    assert_eq!(
        host.state.starts.lock().unwrap().len(),
        1,
        "reusing JupyterLab must not need a second JupyterLab server"
    );

    adapter.abort();
    futures::executor::block_on(service.cancel_for_exit()).unwrap();
}

#[test]
fn switching_from_jupyterlab_to_vscode_restarts_the_protocol_server() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at_with_jupyterlab(
        events,
        directory.path().join("state.db"),
        host,
        0,
        TEST_HANDSHAKE_TIMEOUT,
    )
    .unwrap();
    let package = write_mixed_adapter_course(directory.path());
    futures::executor::block_on(service.open_course(package)).unwrap();
    let adapter = runtime.spawn(fake_adapter_reconnecting(
        directory.path().join("runtime.json"),
    ));

    runtime.block_on(async {
        service
            .begin_learning(LearningMode::ByStep, "ch/jupyter")
            .await
            .unwrap();
        assert_eq!(service.server_adapter(), Some(AdapterId::Jupyterlab));
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;

        service.stop_learning().await.unwrap();
        service.select_unit("ch/vscode").await.unwrap();
        service
            .begin_learning(LearningMode::ByStep, "ch/vscode")
            .await
            .unwrap();
        assert_eq!(service.server_adapter(), Some(AdapterId::Vscode));
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { .. })
        })
        .await;
    });

    adapter.abort();
    futures::executor::block_on(service.cancel_for_exit()).unwrap();
}

#[test]
fn advancing_from_jupyterlab_to_vscode_restarts_the_protocol_server() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at_with_jupyterlab(
        events,
        directory.path().join("state.db"),
        host,
        0,
        TEST_HANDSHAKE_TIMEOUT,
    )
    .unwrap();
    let package = write_mixed_adapter_course(directory.path());
    futures::executor::block_on(service.open_course(package)).unwrap();
    let adapter = runtime.spawn(fake_adapter_reconnecting(
        directory.path().join("runtime.json"),
    ));

    runtime.block_on(async {
        service
            .begin_learning(LearningMode::ByStep, "ch/jupyter")
            .await
            .unwrap();
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { position } if position.unit_path == "ch/jupyter")
        })
        .await;

        service.advance_unit(LearningMode::Continuous).await.unwrap();
        assert_eq!(service.server_adapter(), Some(AdapterId::Vscode));
        wait_for_playback(&mut receiver, |playback| {
            matches!(playback, PlaybackState::UnitComplete { position } if position.unit_path == "ch/vscode")
        })
        .await;
    });

    adapter.abort();
    futures::executor::block_on(service.cancel_for_exit()).unwrap();
}

#[test]
fn a_course_switch_stops_the_old_jupyterlab_server_and_starts_the_new_one() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at_with_jupyterlab(
        events,
        directory.path().join("state.db"),
        host.clone(),
        0,
        TEST_HANDSHAKE_TIMEOUT,
    )
    .unwrap();
    let first = write_adapter_course(&directory.path().join("first"), "jupyterlab");
    let second = write_adapter_course(&directory.path().join("second"), "jupyterlab");

    futures::executor::block_on(service.open_course(first)).unwrap();
    let first_adapter = runtime.spawn(fake_adapter_reconnecting(
        directory.path().join("runtime.json"),
    ));
    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, "ch/main")).unwrap();
    assert_eq!(host.state.starts.lock().unwrap().len(), 1);

    futures::executor::block_on(service.open_course(second)).unwrap();
    assert_eq!(
        host.state.stops.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "switching courses must stop the old server"
    );
    first_adapter.abort();
    let second_adapter = runtime.spawn(fake_adapter_reconnecting(
        directory.path().join("runtime.json"),
    ));
    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, "ch/main")).unwrap();

    let starts = host.state.starts.lock().unwrap().clone();
    assert_eq!(starts.len(), 2);
    assert_ne!(starts[0], starts[1], "the new course has its own server");
    second_adapter.abort();
    futures::executor::block_on(service.cancel_for_exit()).unwrap();
}

#[test]
fn a_missing_extension_is_an_actionable_handshake_error() {
    let directory = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (service, _receiver) =
        jupyterlab_service(directory.path(), host.clone(), 0, Duration::from_millis(50));

    let error =
        futures::executor::block_on(service.begin_learning(LearningMode::ByStep, "ch/main"))
            .unwrap_err();

    assert!(
        error.contains("extension"),
        "the timeout must tell the learner the extension did not connect: {error}"
    );
    assert_eq!(host.state.starts.lock().unwrap().len(), 1);
    assert!(
        host.state.running.load(std::sync::atomic::Ordering::SeqCst),
        "a retry may reuse the still-running server"
    );
}

#[test]
fn a_controlled_server_install_publishes_an_already_connected_adapter() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let runtime_path = directory.path().join("runtime.json");
    let server = runtime
        .block_on(cynapse_client_core::server::start_server_with_path(
            workspace,
            runtime_path.clone(),
            Vec::new(),
        ))
        .unwrap();
    let adapter = runtime.spawn(fake_adapter(runtime_path));

    runtime.block_on(async {
        let mut state = server.state.clone();
        timeout(TEST_HANDSHAKE_TIMEOUT, async {
            loop {
                if matches!(*state.borrow(), ConnectionState::Connected { .. }) {
                    break;
                }
                state.changed().await.unwrap();
            }
        })
        .await
        .expect("the fake adapter did not connect before install");
    });

    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at(events, directory.path().join("state.db")).unwrap();
    {
        let _guard = service.inner._runtime.enter();
        service
            .install_server(server, ReplacementKind::Controlled)
            .unwrap();
    }

    let event = runtime.block_on(async {
        timeout(TEST_HANDSHAKE_TIMEOUT, receiver.recv())
            .await
            .expect("timed out waiting for the initial connection event")
            .expect("event channel closed")
    });
    assert!(matches!(
        event,
        ClientEvent::Connection(ConnectionState::Connected { .. })
    ));

    adapter.abort();
    runtime.block_on(service.stop_server());
}

#[test]
fn a_jupyterlab_start_binds_the_protocol_server_to_the_control_port() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let control_port = listener.local_addr().unwrap().port();
    drop(listener);
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at_with_jupyterlab(
        events,
        directory.path().join("state.db"),
        host,
        control_port,
        TEST_HANDSHAKE_TIMEOUT,
    )
    .unwrap();
    let package = write_adapter_course(directory.path(), "jupyterlab");
    futures::executor::block_on(service.open_course(package)).unwrap();
    while receiver.try_recv().is_ok() {}
    let runtime_path = directory.path().join("runtime.json");
    let adapter = runtime.spawn(fake_adapter_reconnecting(runtime_path.clone()));

    futures::executor::block_on(service.begin_learning(LearningMode::ByStep, "ch/main")).unwrap();

    let discovery: DiscoveryFile =
        serde_json::from_str(&fs::read_to_string(&runtime_path).unwrap()).unwrap();
    assert_eq!(
        discovery.port, control_port,
        "the JupyterLab extension needs the fixed control port"
    );
    while let Ok(event) = receiver.try_recv() {
        assert!(
            !matches!(
                event,
                ClientEvent::Connection(ConnectionState::WaitingForAdapter)
            ),
            "the control-port restart must not look like an Adapter loss"
        );
    }
    adapter.abort();
    futures::executor::block_on(service.cancel_for_exit()).unwrap();
}

#[test]
fn a_control_port_conflict_is_an_actionable_start_error() {
    let directory = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let host = Arc::new(FakeJupyterLabProvisioner {
        state: Arc::new(FakeJupyterLabState::default()),
    });
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = SessionService::new_at_with_jupyterlab(
        events,
        directory.path().join("state.db"),
        host.clone(),
        port,
        Duration::from_millis(50),
    )
    .unwrap();
    let package = write_adapter_course(directory.path(), "jupyterlab");
    futures::executor::block_on(service.open_course(package)).unwrap();

    let error =
        futures::executor::block_on(service.begin_learning(LearningMode::ByStep, "ch/main"))
            .unwrap_err();

    assert!(
        error.contains(&port.to_string()) && error.to_lowercase().contains("in use"),
        "the conflict must name the busy control port: {error}"
    );
    assert!(host.state.starts.lock().unwrap().is_empty());
    drop(listener);
}

#[test]
fn a_jupyterlab_start_is_not_blocked_by_the_browser_handshake_gate() {
    let directory = tempfile::tempdir().unwrap();
    let package = write_adapter_course(directory.path(), "jupyterlab");
    let (mut model, _receiver) = crate::ClientModel::new_at_with_startup_course(
        crate::ClientSettings::default(),
        directory.path().join("state.db"),
        Some(package),
    )
    .unwrap();

    model
        .start_learning(LearningMode::ByStep, "ch/main")
        .expect("SessionService owns the browser handshake");
    model.stop_learning_local();
}
