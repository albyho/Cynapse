use super::*;

#[test]
fn rust_guide_fixture_contains_rustc_and_cargo_steps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rust-book-zh-cn");
    let course = load_package(&root).unwrap();

    assert_eq!(course.manifest.id, "rust_book_zh_cn");
    assert_eq!(course.tree.adapters, vec![AdapterId::Vscode]);
    assert_eq!(course.environment_requirements().len(), 2);
    let playback_steps = course.playback_steps();
    assert_eq!(playback_steps.len(), 9);
    assert_eq!(playback_steps[0].step_id, "hello_world_rustc_1");
    assert_eq!(playback_steps[4].step_id, "hello_world_cargo_1");
    assert_eq!(playback_steps[0].parent_id, "ch01_getting_started");
    assert_eq!(playback_steps[4].parent_id, "ch01_getting_started");

    let commands = course
        .actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::TerminalCommand(command) => Some(command),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        commands
            .iter()
            .map(|command| command.command.as_str())
            .collect::<Vec<_>>(),
        vec![
            "rustc main.rs",
            ".\\main.exe",
            "cargo new hello_cargo",
            "cargo build",
            ".\\target\\debug\\hello_cargo.exe",
            "cargo run",
            "cargo check",
        ]
    );
    assert_eq!(commands[2].cwd, ".");
    assert_eq!(
        commands[2].completion_marker.as_deref(),
        Some("hello_cargo/Cargo.toml")
    );
    assert!(
        commands[3..]
            .iter()
            .all(|command| command.cwd == "hello_cargo")
    );
    assert_eq!(commands[1].expected_output, vec!["Hello, world!"]);
    assert_eq!(commands[4].expected_output, vec!["Hello, world!"]);
    assert!(commands[5].expected_output.is_empty());
    assert!(commands[3].expected_output.is_empty());
    assert!(commands[6].expected_output.is_empty());
}

#[test]
fn rust_guide_cargo_project_is_excluded_from_repository_workspace() {
    let root_manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let manifest = std::fs::read_to_string(root_manifest).unwrap();

    assert!(
        manifest.contains("exclude")
            && manifest.contains("test-data/rust-book-zh-cn/workspace/hello_cargo"),
        "the generated Cargo project must stay outside the repository workspace:\n{manifest}"
    );
}

#[test]
fn controlled_failure_fixture_accepts_expected_compiler_error() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rust-variables-demo");
    let course = load_package(&root).unwrap();
    assert_eq!(course.playback_steps().len(), 6);
    let actions = course.actions();
    assert!(matches!(actions[6], Action::ReplaceText(_)));
    let commands = actions
        .into_iter()
        .filter_map(|action| match action {
            Action::TerminalCommand(command) => Some(command),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(commands.len(), 3);
    assert_eq!(commands[0].accepted_exit_codes, vec![1]);
    assert_eq!(
        commands[0].expected_output,
        vec!["cannot assign twice to immutable variable"]
    );
    assert_eq!(commands[1].accepted_exit_codes, vec![0]);
    assert_eq!(
        commands[2].expected_output,
        vec!["The value of x is: 5", "The value of x is: 6"]
    );
}

#[test]
fn terminal_recovery_fixture_has_long_command_and_timeout_paths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/terminal-recovery-demo");
    let course = load_package(&root).unwrap();
    assert_eq!(course.manifest.id, "terminal_recovery_demo");
    assert_eq!(course.playback_steps().len(), 4);

    let commands = course
        .actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::TerminalCommand(command) => Some(command),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(commands.len(), 3);
    assert!(commands[0].command.contains("Start-Sleep"));
    assert_eq!(commands[0].timeout_ms, 300_000);
    assert_eq!(
        commands[1].expected_output,
        vec!["must not run after interruption"]
    );
    assert_eq!(commands[2].timeout_ms, 1_000);
}

#[tokio::test]
async fn rust_cargo_getting_started_workflow_builds_and_runs() {
    async fn run_with_timeout(
        mut command: tokio::process::Command,
        description: &str,
        deadline: tokio::time::Instant,
    ) -> std::process::Output {
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn().unwrap_or_else(|error| {
            panic!("{description} must be available for the fixture test: {error}")
        });
        tokio::time::timeout_at(deadline, child.wait_with_output())
            .await
            .unwrap_or_else(|_| panic!("{description} timed out after 10 seconds"))
            .unwrap_or_else(|error| panic!("{description} could not be observed: {error}"))
    }

    async fn run_cargo(
        directory: &Path,
        arguments: &[&str],
        deadline: tokio::time::Instant,
    ) -> std::process::Output {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = tokio::process::Command::new(cargo);
        command
            .args(arguments)
            .current_dir(directory)
            .env("CARGO_NET_OFFLINE", "true");
        let output =
            run_with_timeout(command, &format!("cargo {}", arguments.join(" ")), deadline).await;
        assert!(
            output.status.success(),
            "cargo {} failed\nstdout:\n{}\nstderr:\n{}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        output
    }

    let directory = tempfile::tempdir().unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    run_cargo(directory.path(), &["new", "hello_cargo"], deadline).await;

    let project = directory.path().join("hello_cargo");
    let manifest = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("edition = \"2024\""),
        "cargo new must generate an Edition 2024 project:\n{manifest}"
    );

    run_cargo(&project, &["build"], deadline).await;
    #[cfg(windows)]
    let executable = project.join("target").join("debug").join("hello_cargo.exe");
    #[cfg(not(windows))]
    let executable = project.join("target").join("debug").join("hello_cargo");
    assert!(executable.is_file(), "missing {:?}", executable);
    let direct_run = run_with_timeout(
        tokio::process::Command::new(&executable),
        "hello_cargo",
        deadline,
    )
    .await;
    assert!(direct_run.status.success());
    assert!(String::from_utf8_lossy(&direct_run.stdout).contains("Hello, world!"));

    let cargo_run = run_cargo(&project, &["run"], deadline).await;
    assert!(String::from_utf8_lossy(&cargo_run.stdout).contains("Hello, world!"));
    run_cargo(&project, &["check"], deadline).await;
}

#[test]
fn adapter_ids_expose_stable_connection_and_display_names() {
    assert_eq!(AdapterId::Vscode.connection_name(), "vscode");
    assert_eq!(AdapterId::Vscode.display_name(), "VS Code");
    assert_eq!(AdapterId::Jupyterlab.connection_name(), "jupyterlab");
    assert_eq!(AdapterId::Jupyterlab.display_name(), "JupyterLab");
    assert_eq!(
        serde_json::from_value::<AdapterId>(serde_json::json!("jupyterlab")).unwrap(),
        AdapterId::Jupyterlab
    );
}
