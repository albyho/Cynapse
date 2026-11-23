use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_cynapse-content-verify")
}

fn client_testdata(name: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shared = manifest.join("../../course/test-data").join(name);
    if shared.is_dir() {
        shared
    } else {
        manifest.join("../test-data").join(name)
    }
}

#[test]
fn no_content_directories_is_a_successful_skip() {
    let workspace = tempfile::tempdir().unwrap();
    let output = Command::new(binary())
        .current_dir(workspace.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("No content packages found"));
}

#[test]
fn packages_without_narration_audio_report_a_skip_not_an_empty_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    // test-isolation-allow: content verifier owns optional content discovery
    let package = workspace.path().join("courses/package"); // test-isolation-allow: verifier discovery fixture
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "missing_audio",
            "title": "Missing Audio",
            "assets": [{ "id": "missing", "path": "missing.ogg" }]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Skipping"));
    assert!(stdout.contains("skipped because generated narration audio is missing"));
    assert!(!stdout.contains("No content packages found"));
}

#[test]
fn ogg_in_a_title_does_not_skip_package_discovery() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("courses/package"); // test-isolation-allow: verifier discovery fixture
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["title"] = serde_json::json!("title.ogg");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("skipped because generated narration audio is missing"),
        "{stdout}"
    );
}

#[test]
fn intermediate_human_step_reports_unverified_later_steps() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    let step_path = package.join("steps/write_first.json");
    let mut step: serde_json::Value =
        serde_json::from_slice(&fs::read(&step_path).unwrap()).unwrap();
    step["requires_human"] = serde_json::json!(true);
    fs::write(&step_path, serde_json::to_vec_pretty(&step).unwrap()).unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .arg("--unit")
        .arg("ch_multi_step/main")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("later Steps"), "{stderr}");
    assert!(stderr.contains("unverified"), "{stderr}");
}

#[test]
fn allow_human_skips_the_unverified_unit_remainder_with_a_warning() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    let step_path = package.join("steps/write_first.json");
    let mut step: serde_json::Value =
        serde_json::from_slice(&fs::read(&step_path).unwrap()).unwrap();
    step["requires_human"] = serde_json::json!(true);
    fs::write(&step_path, serde_json::to_vec_pretty(&step).unwrap()).unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .arg("--unit")
        .arg("ch_multi_step/main")
        .arg("--allow-human")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warning: skipping rest of Unit"),
        "{stdout}"
    );
    assert!(stdout.contains("later Steps are unverified"), "{stdout}");
}

#[test]
fn failed_validation_removes_the_artifact_root() {
    let workspace = tempfile::tempdir().unwrap();
    let artifact_parent = workspace.path().join("artifacts");
    fs::create_dir_all(&artifact_parent).unwrap();
    let missing = workspace.path().join("missing-package");

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .env("TEMP", &artifact_parent)
        .env("TMP", &artifact_parent)
        .env("TMPDIR", &artifact_parent)
        .arg("--course")
        .arg(&missing)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let leftovers: Vec<_> = fs::read_dir(&artifact_parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        leftovers.is_empty(),
        "failed validation left artifact directories: {leftovers:?}"
    );
}

#[test]
fn since_rejects_git_option_injection_without_writing_files() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    init_git_repo(&package);

    let injected = workspace.path().join("injected.diff");
    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .arg("--changed")
        .arg("--since")
        .arg(format!("--output={}", injected.display()))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !injected.exists(),
        "git option injection wrote {}",
        injected.display()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--since"), "{stderr}");
    assert!(stderr.contains("must not start with '-'"), "{stderr}");
}

#[test]
fn since_accepts_a_valid_revision_after_end_of_options() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    init_git_repo(&package);

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .arg("--changed")
        .arg("--since")
        .arg("HEAD")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("No affected Units"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn human_final_step_at_a_unit_boundary_does_not_fail_whole_course_validation() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("human-final-unit");
    copy_directory(&client_testdata("presentation-demo"), &package);
    clear_workspace_state(&package);

    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let chapter = manifest["children"][0]["children"].as_array_mut().unwrap();
    let first_unit = &mut chapter[0];
    let step_file = first_unit["steps"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["step_file"]
        .as_str()
        .unwrap()
        .to_string();
    let step_path = package.join(&step_file);
    let mut final_step: serde_json::Value =
        serde_json::from_slice(&fs::read(&step_path).unwrap()).unwrap();
    final_step["requires_human"] = serde_json::json!(true);
    fs::write(&step_path, serde_json::to_vec_pretty(&final_step).unwrap()).unwrap();

    fs::write(
        package.join("steps/next.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "step_id": "next",
            "title": "Next step",
            "actions": [{ "type": "Present" }]
        }))
        .unwrap(),
    )
    .unwrap();
    chapter.push(serde_json::json!({
        "id": "next",
        "title": "Next Unit",
        "adapters": [],
        "environment": [],
        "steps": [{ "step_file": "steps/next.json" }]
    }));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Validated"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn explicit_missing_course_is_an_actionable_error() {
    let workspace = tempfile::tempdir().unwrap();
    let missing = workspace.path().join("missing-course");
    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&missing)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing-course"));
    assert!(stderr.contains("manifest.json"));
}

#[test]
fn validates_a_package_without_manifest_workspace_and_removes_the_default_root() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    fs::create_dir_all(package.join("steps")).unwrap();
    fs::write(
        package.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "default_workspace",
            "title": "Default Workspace",
            "adapters": ["vscode"],
            "children": [{
                "id": "ch",
                "title": "Grouping",
                "children": [{
                    "id": "unit",
                    "title": "Unit",
                    "steps": [{"step_file": "steps/step.json"}]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        package.join("steps/step.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "step",
            "title": "Step",
            "actions": [{"type": "CreateFile", "path": "done.txt"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!package.join("workspace").exists());
}

#[test]
fn validates_an_explicit_package_without_mutating_it() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    let before = snapshot(&package);
    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validated"));
    assert_eq!(snapshot(&package), before);
}

#[test]
fn validates_a_tree_package_and_reports_unit_paths() {
    let workspace = tempfile::tempdir().unwrap();
    let package = client_testdata("flexible-tree-demo");
    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Validated"));
    assert!(stdout.contains("intro"));
    assert!(stdout.contains("ch02_advanced/ch02_02_abstraction/structural_role"));
}

#[test]
fn discovers_only_packages_present_in_content_directories() {
    let workspace = tempfile::tempdir().unwrap();
    let courses_root = workspace.path().join("courses"); // test-isolation-allow: temporary content root
    let discovered = courses_root.join("discovered-demo");
    copy_directory(&client_testdata("multi-step-demo"), &discovered);
    fs::create_dir_all(courses_root.join("not-a-package")).unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Validated"));
    assert!(stdout.contains("discovered-demo"));
}

#[test]
fn explicitly_ignored_packages_are_skipped_during_discovery() {
    let workspace = tempfile::tempdir().unwrap();
    let courses_root = workspace.path().join("courses"); // test-isolation-allow: temporary content root
    copy_directory(
        &client_testdata("multi-step-demo"),
        &courses_root.join("included-demo"),
    );
    copy_directory(
        &client_testdata("multi-step-demo"),
        &courses_root.join("ignored-demo"),
    );

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .args(["--ignore", "courses/ignored-demo"]) // test-isolation-allow: temporary content root
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("included-demo"));
    assert!(!stdout.contains("ignored-demo"));
}

#[test]
fn explicit_course_validation_is_not_bypassed_by_ignore() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("legacy-demo");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["children"][0]["children"][0]["narration"] =
        serde_json::json!({ "segments": [], "step_gates": [] });
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .args(["--ignore", "legacy-demo", "--course", "legacy-demo"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown field `narration`"));
}

#[test]
fn rejects_invalid_caption_markup_before_running_scenarios() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("bad-caption");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    let step_path = package.join("steps/write_first.json");
    let mut step: serde_json::Value =
        serde_json::from_slice(&fs::read(&step_path).unwrap()).unwrap();
    step["actions"][0]["narration"] = serde_json::json!({
        "before": {
            "clips": [{ "caption": "<c.aqua>未闭合", "duration_ms": 900 }]
        }
    });
    fs::write(&step_path, serde_json::to_vec_pretty(&step).unwrap()).unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .args(["--course", "bad-caption"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid caption markup"));
    assert!(stderr.contains("unclosed"));
    assert!(stderr.contains("write_first"));
}

#[test]
fn reports_all_package_errors_in_one_run() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("all-errors");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert("adapters".to_string(), serde_json::json!([]));
    unit.insert("environment".to_string(), serde_json::json!([]));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let step_path = package.join("steps/write_first.json");
    let mut step: serde_json::Value =
        serde_json::from_slice(&fs::read(&step_path).unwrap()).unwrap();
    step["actions"][0]["narration"] = serde_json::json!({
        "before": {
            "clips": [{ "caption": "<c.aqua>未闭合", "duration_ms": 900 }]
        }
    });
    fs::write(&step_path, serde_json::to_vec_pretty(&step).unwrap()).unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .args(["--course", "all-errors"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid caption markup"), "{stderr}");
    assert!(
        stderr.contains("invalid adapters/environment configuration"),
        "{stderr}"
    );
}

#[test]
fn rejects_empty_adapters_for_non_presentation_units() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("invalid-adapters");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert("adapters".to_string(), serde_json::json!([]));
    unit.insert("environment".to_string(), serde_json::json!([]));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .args(["--course", "invalid-adapters"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid adapters/environment configuration"));
    assert!(stderr.contains("ch_multi_step/main"));
    assert!(stderr.contains("non-Present"));
}

#[test]
fn accepts_local_only_presentation_units() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("presentation-only");
    copy_directory(&client_testdata("presentation-demo"), &package);
    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert("adapters".to_string(), serde_json::json!([]));
    unit.insert("environment".to_string(), serde_json::json!([]));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .args(["--course", "presentation-only"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Local-only units"));
    assert!(stdout.contains("ch_presentation/pure_presentation"));
}

#[test]
fn rejects_environment_checks_without_an_app_adapter() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("environment-without-adapter");
    copy_directory(&client_testdata("presentation-demo"), &package);
    let manifest_path = package.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let unit = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap();
    unit.insert("adapters".to_string(), serde_json::json!([]));
    unit.insert(
        "environment".to_string(),
        serde_json::json!([{
            "id": "rustc",
            "command": "rustc --version"
        }]),
    );
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .args(["--course", "environment-without-adapter"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ch_presentation/pure_presentation"));
    assert!(stderr.contains("environment checks (rustc)"));
}

#[cfg(windows)]
fn make_junction(link: &Path, target: &Path) {
    let output = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn content_verify_rejects_a_junction_ancestor() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    fs::create_dir_all(package.join("workspace")).unwrap();

    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside.path().join("main")).unwrap();
    fs::write(outside.path().join("main/keep.txt"), "do not delete").unwrap();
    make_junction(
        &package.join("workspace").join("ch_multi_step"),
        outside.path(),
    );

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .arg("--unit")
        .arg("ch_multi_step/main")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("symbolic link or junction"));
    assert!(outside.path().join("main/keep.txt").exists());
}

#[cfg(unix)]
#[test]
fn content_verify_rejects_a_symlink_ancestor() {
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("package");
    copy_directory(&client_testdata("multi-step-demo"), &package);
    clear_workspace_state(&package);
    fs::create_dir_all(package.join("workspace")).unwrap();

    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside.path().join("main")).unwrap();
    fs::write(outside.path().join("main/keep.txt"), "do not delete").unwrap();
    std::os::unix::fs::symlink(outside.path(), package.join("workspace/ch_multi_step")).unwrap();

    let output = Command::new(binary())
        .current_dir(workspace.path())
        .arg("--course")
        .arg(&package)
        .arg("--unit")
        .arg("ch_multi_step/main")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("symbolic link or junction"));
    assert!(outside.path().join("main/keep.txt").exists());
}

fn init_git_repo(package: &Path) {
    for arguments in [
        &["init"][..],
        &["config", "user.email", "ci@example.com"][..],
        &["config", "user.name", "CI"][..],
        &["add", "."][..],
        &["commit", "-m", "baseline"][..],
    ] {
        let status = Command::new("git")
            .current_dir(package)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success(), "git {arguments:?} failed");
    }
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Removes workspace files left by earlier local runs while keeping `.gitkeep`.
fn clear_workspace_state(package: &Path) {
    let workspace = package.join("workspace");
    let entries = match fs::read_dir(&workspace) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries {
        let entry = entry.unwrap();
        if entry.file_name() == ".gitkeep" {
            continue;
        }
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else {
                files.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}
