use super::*;
use cynapse_client_i18n::t_with;

fn wait_with_during_audio(duration_ms: u64, clip_duration_ms: u64) -> PlaybackAction {
    PlaybackAction {
        action: Action::Wait(crate::course::WaitAction { duration_ms }),
        step_id: "step".to_string(),
        narration: ActionNarration {
            during: Some(ActionNarrationPhaseClips {
                clips: vec![ActionNarrationClip {
                    audio_path: Some(std::path::PathBuf::from("wait.ogg")),
                    audio_relative_path: Some(std::path::PathBuf::from("wait.ogg")),
                    offset_ms: Some(250),
                    duration_ms: clip_duration_ms,
                    caption: Some("Watch the output".to_string()),
                }],
            }),
            ..ActionNarration::default()
        },
    }
}

fn after_wait() -> PlaybackAction {
    PlaybackAction {
        action: insert_text("after wait"),
        step_id: "step".to_string(),
        narration: ActionNarration::default(),
    }
}

#[tokio::test]
async fn wait_audio_shorter_than_the_timer_keeps_the_authored_wait() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![wait_with_during_audio(500, 80), after_wait()],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle.commands.narration_started(clip.clone()).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.commands.narration_ended(clip).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(200), socket.next())
            .await
            .is_err(),
        "a shorter Wait audio clip must not shorten the authored Wait"
    );

    let next = tokio::time::timeout(Duration::from_secs(1), receive_action(&mut socket))
        .await
        .expect("the Wait should complete at its authored duration");
    assert_eq!(next.payload["text"], "after wait");

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn wait_audio_longer_than_the_timer_extends_the_effective_wait() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![wait_with_during_audio(120, 700), after_wait()],
    )
    .await;
    let mut playback = handle.playback.clone();
    let started = tokio::time::Instant::now();

    handle.commands.play().unwrap();
    let clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle.commands.narration_started(clip.clone()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(250), socket.next())
            .await
            .is_err(),
        "a longer Wait audio clip must extend the effective Wait"
    );
    handle.commands.narration_ended(clip).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(150), socket.next())
            .await
            .is_err(),
        "ending the clip early must not shorten the effective Wait"
    );

    let next = tokio::time::timeout(Duration::from_secs(1), receive_action(&mut socket))
        .await
        .expect("the Wait should complete after the effective duration");
    assert_eq!(next.payload["text"], "after wait");
    assert!(started.elapsed() >= Duration::from_millis(600));

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn wait_timer_completes_a_started_audio_clip_that_never_ends() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![wait_with_during_audio(80, 250), after_wait()],
    )
    .await;
    let mut playback = handle.playback.clone();
    let started = tokio::time::Instant::now();

    handle.commands.play().unwrap();
    let clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle.commands.narration_started(clip).unwrap();

    let next = tokio::time::timeout(Duration::from_secs(1), receive_action(&mut socket))
        .await
        .expect("a Wait audio clip that starts but never ends must not hang the Wait");
    assert_eq!(next.payload["text"], "after wait");
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "the effective deadline must still include the audio duration"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn wait_progress_label_uses_the_effective_duration() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, _socket) =
        one_step_server(directory.path(), vec![wait_with_during_audio(300, 900)]).await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle.commands.narration_started(clip).unwrap();

    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(
                state,
                PlaybackState::Playing { action_label, .. }
                    if action_label == &t_with("action.wait", &[("ms", "900")])
            )
        }),
    )
    .await
    .expect("the Wait progress label should show the effective duration");

    handle.shutdown().await;
}

#[tokio::test]
async fn replay_skips_wait_audio_instead_of_gating_the_action() {
    let directory = tempfile::tempdir().unwrap();
    let mut step = playback_step(
        "write",
        "Write",
        vec![Action::Wait(crate::course::WaitAction {
            duration_ms: 10_000,
        })],
    );
    step.actions[0].narration = wait_with_during_audio(10_000, 2_000).narration;
    let handle = start_server_with_path_and_resume(
        directory.path().join("workspace"),
        directory.path().join("runtime.json"),
        vec![step],
        Some(PlaybackResume {
            unit_path: "main_unit".to_string(),
            step_id: "write".to_string(),
            step_started: true,
            action_index: Some(0),
            recovery: None,
        }),
    )
    .await
    .unwrap();
    let (mut socket, discovery) = connect(&handle).await;
    send_hello(&mut socket, discovery.token).await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_playback(&mut playback, |state| {
            matches!(state, PlaybackState::UnitComplete { .. })
        }),
    )
    .await
    .expect("replay must complete the Wait without starting its audio");

    handle.shutdown().await;
}

#[tokio::test]
async fn forced_skip_waits_skips_wait_audio_without_narrating() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![wait_with_during_audio(10_000, 2_000), after_wait()],
    )
    .await;

    handle.commands.set_skip_waits(true).unwrap();
    handle.commands.play().unwrap();
    let next = tokio::time::timeout(Duration::from_secs(1), receive_action(&mut socket))
        .await
        .expect("skip_waits must not wait for Wait audio");
    assert_eq!(next.payload["text"], "after wait");

    handle.shutdown().await;
}

#[tokio::test]
async fn wait_audio_failure_reports_the_narration_error_and_cancels_the_action() {
    let directory = tempfile::tempdir().unwrap();
    let (handle, mut socket) = one_step_server(
        directory.path(),
        vec![wait_with_during_audio(300, 1_500), after_wait()],
    )
    .await;
    let mut playback = handle.playback.clone();

    handle.commands.play().unwrap();
    let clip = wait_for_narrating(&mut playback, ActionNarrationPhase::During, 0, 0).await;
    handle.commands.narration_started(clip.clone()).unwrap();
    handle
        .commands
        .narration_failed(clip, "audio decode failed")
        .unwrap();
    wait_for_playback(&mut playback, |state| {
        matches!(
            state,
            PlaybackState::Error {
                code,
                ..
            } if code == "narration_failed"
        )
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        matches!(&*playback.borrow(), PlaybackState::Error { .. }),
        "a failed Wait audio clip must not complete the timer into progress"
    );
    // The existing narration-failure path cancels the (local) active Action.
    let cancel = tokio::time::timeout(Duration::from_millis(200), receive_envelope(&mut socket))
        .await
        .expect("the narration failure path should cancel the active action");
    assert_eq!(cancel.kind, MessageType::Cancel);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "a failed Wait audio clip must not dispatch the next Action"
    );

    socket.close(None).await.unwrap();
    handle.shutdown().await;
}
