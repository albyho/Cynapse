use super::*;

fn units() -> Vec<(String, String)> {
    ["ch1", "ch1/sec1", "ch2"]
        .iter()
        .map(|path| (path.to_string(), path.to_string()))
        .collect()
}

fn selection(paths: &[&str]) -> Affected {
    let changed = paths
        .iter()
        .map(|path| path.to_string())
        .collect::<Vec<_>>();
    affected_unit_paths(&units(), &changed)
}

fn names(paths: &[&str]) -> Affected {
    Affected::Units(paths.iter().map(|path| path.to_string()).collect())
}

#[test]
fn a_step_in_a_subtree_selects_that_subtree_only() {
    assert_eq!(
        selection(&["steps/ch1/sec1/observe.json"]),
        names(&["ch1/sec1"])
    );
}

#[test]
fn a_step_above_the_subtree_selects_the_whole_subtree() {
    assert_eq!(
        selection(&["steps/ch1/intro.json"]),
        names(&["ch1", "ch1/sec1"])
    );
}

#[test]
fn a_step_at_the_steps_root_selects_every_unit() {
    assert_eq!(
        selection(&["steps/shared.json"]),
        names(&["ch1", "ch1/sec1", "ch2"])
    );
}

#[test]
fn a_seed_change_selects_only_its_own_unit() {
    assert_eq!(
        selection(&["seeds/ch1/sec1/input.txt"]),
        names(&["ch1/sec1"])
    );
}

#[test]
fn package_wide_inputs_select_every_unit() {
    for path in [
        "manifest.json",
        "resources/agent-desktop/src/lib.rs",
        "assets/narration/a.ogg",
    ] {
        assert_eq!(selection(&[path]), Affected::All, "{path}");
    }
}

#[test]
fn unrelated_files_select_nothing() {
    assert_eq!(
        selection(&["README.md", "authoring/audio-tracks/ch1.json"]),
        names(&[])
    );
}

#[test]
fn shared_contract_changes_select_every_package() {
    fn git(repo: &Path, arguments: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path();
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    std::fs::create_dir_all(repo.join("course/src")).unwrap();
    std::fs::write(repo.join("course/src/loader.rs"), "fn main() {}\n").unwrap();
    std::fs::create_dir_all(repo.join("package")).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "initial"]);

    let package = repo.join("package");
    assert!(
        shared_contract_changes(&package, "HEAD")
            .unwrap()
            .is_empty()
    );

    std::fs::create_dir_all(repo.join("client/core/src")).unwrap();
    std::fs::write(
        repo.join("client/core/src/course_session.rs"),
        "pub fn run() {}\n",
    )
    .unwrap();
    assert_eq!(
        shared_contract_changes(&package, "HEAD").unwrap(),
        vec!["client/core/src/course_session.rs".to_string()]
    );

    git(repo, &["add", "client/core/src/course_session.rs"]);
    git(repo, &["commit", "-m", "change playback"]);
    std::fs::write(
        repo.join("client/core/src/window_position.rs"),
        "pub fn clamp() {}\n",
    )
    .unwrap();
    assert!(
        shared_contract_changes(&package, "HEAD")
            .unwrap()
            .is_empty()
    );

    std::fs::write(
        repo.join("course/src/loader.rs"),
        "fn main() { let _ = 1; }\n",
    )
    .unwrap();
    assert_eq!(
        shared_contract_changes(&package, "HEAD").unwrap(),
        vec!["course/src/loader.rs".to_string()]
    );

    git(repo, &["add", "course/src/loader.rs"]);
    git(repo, &["commit", "-m", "change loader"]);
    std::fs::write(repo.join("package/README.md"), "package\n").unwrap();
    assert!(
        shared_contract_changes(&package, "HEAD")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn detects_missing_generated_narration_audio() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        br#"{"assets":[{"path":"assets/narration/a.ogg"}]}"#,
    )
    .unwrap();
    assert!(missing_narration_audio(directory.path()));

    std::fs::create_dir_all(directory.path().join("assets/narration")).unwrap();
    std::fs::write(
        directory.path().join("assets/narration/a.ogg"),
        b"not really ogg",
    )
    .unwrap();
    assert!(!missing_narration_audio(directory.path()));
}

#[test]
fn packages_without_audio_are_not_skipped() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        br#"{"children":[]}"#,
    )
    .unwrap();
    assert!(!missing_narration_audio(directory.path()));
}
