use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
fn unique_instance_suffix() -> String {
    format!(
        "-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[cfg(windows)]
fn wait_for_exit(mut child: Child, seconds: u64) -> (Option<i32>, String) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("client did not exit within {seconds}s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let output = child.wait_with_output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code(), text)
}

#[cfg(windows)]
#[test]
fn a_broken_state_database_exits_without_opening_a_window() {
    let directory = tempfile::tempdir().unwrap();
    let local_app_data = directory.path().join("local");
    let app_data = directory.path().join("appdata");
    std::fs::create_dir_all(local_app_data.join("Cynapse/Client")).unwrap();
    std::fs::create_dir_all(app_data.join("com.cynapse.client")).unwrap();
    std::fs::write(
        local_app_data.join("Cynapse/Client/state.db"),
        "not a sqlite database",
    )
    .unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_cynapse-client"))
        .env("LOCALAPPDATA", &local_app_data)
        .env("APPDATA", &app_data)
        .env("CYNAPSE_LOG", "error")
        .env("CYNAPSE_TEST_NO_DIALOG", "1")
        .env("CYNAPSE_TEST_INSTANCE_SUFFIX", unique_instance_suffix())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let (code, text) = wait_for_exit(child, 10);
    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("state.db"), "{text}");
}

#[cfg(windows)]
#[test]
fn a_single_instance_failure_reports_the_error_instead_of_exiting_quietly() {
    let directory = tempfile::tempdir().unwrap();
    let local_app_data = directory.path().join("local");
    let app_data = directory.path().join("appdata");
    std::fs::create_dir_all(local_app_data.join("Cynapse/Client")).unwrap();
    std::fs::create_dir_all(app_data.join("com.cynapse.client")).unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_cynapse-client"))
        .env("LOCALAPPDATA", &local_app_data)
        .env("APPDATA", &app_data)
        .env("CYNAPSE_LOG", "error")
        .env("CYNAPSE_TEST_NO_DIALOG", "1")
        .env("CYNAPSE_TEST_INSTANCE_SUFFIX", unique_instance_suffix())
        // The guard is a Win32 object, so the test cannot make it fail on
        // demand; the seam feeds the same `Acquire::Failed` arm main.rs sees.
        .env("CYNAPSE_TEST_INSTANCE_FAILURE", "guard unavailable")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let (code, text) = wait_for_exit(child, 10);
    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("guard unavailable"), "{text}");
}
