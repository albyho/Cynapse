use super::super::CourseSessionState;

#[cfg(windows)]
pub(super) fn make_junction(link: &std::path::Path, target: &std::path::Path) {
    let output = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create test junction: link={} target={} stdout={} stderr={}",
        link.display(),
        target.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn copy_directory(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    if source.file_name().is_some_and(|name| name == "workspace") {
        let keep = source.join(".gitkeep");
        if keep.exists() {
            std::fs::copy(keep, destination.join(".gitkeep")).unwrap();
        }
        return;
    }
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub(super) fn fixture_path(name: &str) -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shared = manifest.join("../../course/test-data").join(name);
    if shared.is_dir() {
        shared
    } else {
        manifest.join("../test-data").join(name)
    }
}

pub(super) fn copied_fixture(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join(name);
    copy_directory(&fixture_path(name), &destination);
    (directory, destination)
}

/// Compact failure diff for open/restore atomicity tests: the full tree is too
/// noisy for `assert_eq!`, while the DB row counts are asserted separately.
pub(super) fn assert_open_state_unchanged(
    expected: &CourseSessionState,
    actual: &CourseSessionState,
) {
    let summary = |state: &CourseSessionState| {
        (
            state.active_course_path.clone(),
            state.active_course.as_ref().map(|course| {
                (
                    course.id.clone(),
                    course.version.clone(),
                    course.has_progress(),
                    course.current_unit_path.clone(),
                    course.current_step_id.clone(),
                )
            }),
            state.selected_unit.as_ref().map(|unit| unit.path.clone()),
            state
                .recent_courses
                .iter()
                .map(|course| course.path.clone())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(summary(expected), summary(actual));
}

/// The canonical form the session stores. Tests must not compare against the
/// raw temp path: on CI Windows hands back an 8.3 short name (RUNNER~1) while
/// the session canonicalizes to the long path.
pub(super) fn stored_path(path: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    super::super::normalize_package_path(path.as_ref()).unwrap()
}
