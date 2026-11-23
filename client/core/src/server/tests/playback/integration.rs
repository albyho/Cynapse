use super::*;

/// End-to-end fixture walkthroughs.

#[tokio::test]
async fn rust_guide_cargo_workflow_completes() {
    let directory = tempfile::tempdir().unwrap();
    let package_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../course/test-data/rust-book-zh-cn");
    let course = crate::course::load_package(&package_root).unwrap();
    let workspace = course.workspace_path();
    let steps = course.playback_steps();
    let handle = start_server_with_path(workspace, directory.path().join("runtime.json"), steps)
        .await
        .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;

    let mut playback = handle.playback.clone();
    let PlaybackState::Ready {
        position: Some(position),
    } = playback.borrow().clone()
    else {
        panic!("expected ready playback");
    };
    assert_eq!(position.overall_action_total, 11);
    assert_eq!(position.step_total, 9);

    handle.commands.play().unwrap();
    let create = receive_action(&mut socket).await;
    assert_eq!(create.payload["type"], "CreateFile");
    send_ack(&mut socket, &create.id, "done").await;
    let open = receive_action(&mut socket).await;
    assert_eq!(open.payload["type"], "OpenFile");
    send_ack(&mut socket, &open.id, "done").await;
    let insert = receive_action(&mut socket).await;
    assert_eq!(insert.payload["type"], "InsertText");
    send_ack(&mut socket, &insert.id, "done").await;
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete { position, .. }
                if position.step_id == "hello_world_rustc_1"
        )
    })
    .await;

    handle.commands.play().unwrap();
    let ensure = receive_action(&mut socket).await;
    assert_eq!(ensure.payload["type"], "TerminalEnsure");
    send_ack(&mut socket, &ensure.id, "done").await;
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete { position, .. }
                if position.step_id == "hello_world_rustc_2"
        )
    })
    .await;

    handle.commands.play().unwrap();
    let rustc = receive_action(&mut socket).await;
    assert_eq!(rustc.payload["command"], "rustc main.rs");
    send_terminal_ack(&mut socket, rustc.id, 0, "").await;
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete { position, .. }
                if position.step_id == "hello_world_rustc_3"
        )
    })
    .await;

    handle.commands.play().unwrap();
    let rustc_run = receive_action(&mut socket).await;
    assert_eq!(rustc_run.payload["command"], ".\\main.exe");
    send_terminal_ack(&mut socket, rustc_run.id, 0, "Hello, world!\r\n").await;

    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::StepComplete {
                    position: PlaybackPosition {
                        step_id,
                        overall_action_index: 6,
                        overall_action_total: 11,
                        ..
                    },
                    ..
                } if step_id == "hello_world_rustc_4"
            )
        }),
    )
    .await
    .unwrap();

    handle.commands.play().unwrap();
    let cargo_new = receive_action(&mut socket).await;
    assert_eq!(cargo_new.payload["type"], "TerminalCommand");
    assert_eq!(cargo_new.payload["command"], "cargo new hello_cargo");
    assert_eq!(cargo_new.payload["cwd"], ".");
    assert_eq!(
        cargo_new.payload["completion_marker"],
        "hello_cargo/Cargo.toml"
    );
    send_terminal_ack(&mut socket, cargo_new.id, 0, "").await;

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete { position, .. }
                if position.step_id == "hello_world_cargo_1"
        )
    })
    .await;
    handle.commands.play().unwrap();
    let cargo_build = receive_action(&mut socket).await;
    assert_eq!(cargo_build.payload["command"], "cargo build");
    assert_eq!(cargo_build.payload["cwd"], "hello_cargo");
    send_terminal_ack(&mut socket, cargo_build.id, 0, "").await;

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete { position, .. }
                if position.step_id == "hello_world_cargo_2"
        )
    })
    .await;
    handle.commands.play().unwrap();
    let cargo_run_binary = receive_action(&mut socket).await;
    assert_eq!(
        cargo_run_binary.payload["command"],
        ".\\target\\debug\\hello_cargo.exe"
    );
    send_terminal_ack(&mut socket, cargo_run_binary.id, 0, "Hello, world!\r\n").await;

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete { position, .. }
                if position.step_id == "hello_world_cargo_3"
        )
    })
    .await;
    handle.commands.play().unwrap();
    let cargo_run = receive_action(&mut socket).await;
    assert_eq!(cargo_run.payload["command"], "cargo run");
    send_terminal_ack(&mut socket, cargo_run.id, 0, "Hello, world!\r\n").await;

    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::StepComplete { position, .. }
                if position.step_id == "hello_world_cargo_4"
        )
    })
    .await;
    handle.commands.play().unwrap();
    let cargo_check = receive_action(&mut socket).await;
    assert_eq!(cargo_check.payload["command"], "cargo check");
    send_terminal_ack(&mut socket, cargo_check.id, 0, "").await;

    tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::UnitComplete {
                    position: PlaybackPosition {
                        parent_id,
                        overall_action_index: 11,
                        overall_action_total: 11,
                        ..
                    }
                } if parent_id == "ch01_getting_started"
            )
        }),
    )
    .await
    .unwrap();

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
