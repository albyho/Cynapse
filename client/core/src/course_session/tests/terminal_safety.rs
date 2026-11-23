#[cfg(windows)]
#[test]
fn unit_reset_leaves_a_locked_empty_unit_directory_in_place() {
    use super::super::*;
    use super::support::copy_directory;
    use std::path::PathBuf;

    let directory = tempfile::tempdir().unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-data/challenge-fix-test");
    let package = directory.path().join("course");
    copy_directory(&fixture, &package);
    let workspace = package.join("workspace");
    let unit_workspace = workspace.join("ch/main");
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/main", true).unwrap();
    session
        .record_progress("ch/main", "fix_add", false)
        .unwrap();

    // Hold the Unit directory as a process cwd. Reset must clear the contents
    // and leave the empty directory in place rather than failing or waiting.
    let mut holder = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "ping -n 10 127.0.0.1 >nul"])
        .current_dir(&unit_workspace)
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));

    let reset = session.reset_unit("ch/main", true);
    let _ = holder.kill();
    let _ = holder.wait();

    assert!(reset.is_ok(), "reset failed: {reset:?}");
    assert!(unit_workspace.is_dir());
    assert_eq!(std::fs::read_dir(&unit_workspace).unwrap().count(), 0);
}
