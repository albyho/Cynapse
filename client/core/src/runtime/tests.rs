use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;

struct FakeRunner {
    requests: RefCell<Vec<CommandRequest>>,
    responses: RefCell<VecDeque<std::io::Result<CommandResult>>>,
}

impl FakeRunner {
    fn new(responses: Vec<std::io::Result<CommandResult>>) -> Self {
        Self {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into()),
        }
    }

    fn requests(&self) -> Vec<CommandRequest> {
        self.requests.borrow().clone()
    }

    fn clear(&self) {
        self.requests.borrow_mut().clear();
    }

    fn enqueue(&self, responses: Vec<std::io::Result<CommandResult>>) {
        self.responses.borrow_mut().extend(responses);
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, request: &CommandRequest) -> std::io::Result<CommandResult> {
        self.requests.borrow_mut().push(request.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| panic!("unexpected command: {request:?}"))
    }
}

fn success(stdout: &str) -> std::io::Result<CommandResult> {
    Ok(CommandResult {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    })
}

fn failure(stderr: &str) -> std::io::Result<CommandResult> {
    Ok(CommandResult {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_string(),
    })
}

fn missing_program() -> std::io::Result<CommandResult> {
    Err(std::io::Error::from(std::io::ErrorKind::NotFound))
}

fn course(directory: &Path) -> (PathBuf, PathBuf) {
    let package = directory.join("course");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("pyproject.toml"),
        "[project]\nname = \"acceptance\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\ndependencies = []\n",
    )
    .unwrap();
    std::fs::write(
        package.join("pylock.toml"),
        "lock-version = \"1.0\"\ncreated-by = \"test\"\npackages = []\n",
    )
    .unwrap();
    let workspace = package.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    (package, workspace)
}

const EXTENSION_WHEEL: &str = "wheels/cynapse_jupyterlab_extension-0.2.4-py3-none-any.whl";

fn baseline() -> CourseEnvironmentBaseline {
    CourseEnvironmentBaseline::new("0.2.4", EXTENSION_WHEEL)
}

fn manager<'a>(runner: &'a FakeRunner, package: &Path, workspace: &Path) -> RuntimeManager<'a> {
    RuntimeManager::new(runner, package, workspace, baseline())
}

#[test]
fn missing_learner_python_is_a_typed_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        missing_program(),
        missing_program(),
        missing_program(),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeSetupError::PythonNotFound { required }
                if required == PythonVersion { major: 3, minor: 10 }
        ),
        "{error}"
    );
    assert_eq!(runner.requests().len(), 3);
    let requests = runner.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.program.as_str())
            .collect::<Vec<_>>(),
        vec!["python3", "python", "py"]
    );
    assert!(requests.iter().all(|request| {
        request.cwd == workspace && request.args.last().is_some_and(|arg| arg == "--version")
    }));
}

#[test]
fn an_incompatible_python_reports_both_versions() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.9.7\n"),
        success("Python 3.9.7\n"),
        missing_program(),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeSetupError::PythonIncompatible { found, required }
                if found == PythonVersion { major: 3, minor: 9 }
                    && required == PythonVersion { major: 3, minor: 10 }
        ),
        "{error}"
    );
}

#[test]
fn a_course_can_require_a_newer_python() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    std::fs::write(
        package.join("pyproject.toml"),
        "[project]\nrequires-python = \">=3.12\"\ndependencies = []\n",
    )
    .unwrap();
    let runner = FakeRunner::new(vec![
        success("Python 3.11.9\n"),
        missing_program(),
        missing_program(),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeSetupError::PythonIncompatible { required, .. }
                if required == PythonVersion { major: 3, minor: 12 }
        ),
        "{error}"
    );
}

#[test]
fn detection_prefers_a_compatible_python_over_an_earlier_incompatible_one() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.7.9\n"),
        success("Python 3.13.1\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);

    let environment = manager(&runner, &package, &workspace).provision().unwrap();

    assert_eq!(
        environment.python,
        PythonVersion {
            major: 3,
            minor: 13
        }
    );
    assert_eq!(
        environment.interpreter,
        venv_interpreter(&course_environment_dir(&workspace))
    );
}

fn install_interpreter(workspace: &Path) {
    let interpreter = venv_interpreter(&course_environment_dir(workspace));
    std::fs::create_dir_all(interpreter.parent().unwrap()).unwrap();
    std::fs::write(&interpreter, "").unwrap();
}

#[test]
fn fresh_provisioning_installs_baseline_then_course_under_constraints() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);

    let environment = manager(&runner, &package, &workspace).provision().unwrap();

    let venv = course_environment_dir(&workspace);
    let interpreter = venv_interpreter(&venv);
    assert_eq!(environment.interpreter, interpreter);
    assert_eq!(
        environment.python,
        PythonVersion {
            major: 3,
            minor: 12
        }
    );

    let requests = runner.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests[0],
        CommandRequest::new("python3", ["--version"], &workspace)
    );
    assert_eq!(
        requests[1],
        CommandRequest::new(
            "python3",
            ["-m", "venv", venv.display().to_string().as_str()],
            &workspace
        )
    );

    let constraints = venv.join(STATE_DIR).join(CONSTRAINTS_FILE);
    let constraints_text = std::fs::read_to_string(&constraints).unwrap();
    assert!(
        constraints_text.contains("jupyterlab==4.6.6\n"),
        "{constraints_text}"
    );
    assert!(
        constraints_text.contains("cynapse-jupyterlab-extension==0.2.4\n"),
        "{constraints_text}"
    );

    let pip_args = vec![
        "-m".to_string(),
        "pip".to_string(),
        "install".to_string(),
        "--disable-pip-version-check".to_string(),
        "--no-input".to_string(),
        "--constraint".to_string(),
        constraints.display().to_string(),
        "pip>=26.1".to_string(),
    ];
    assert_eq!(requests[2].program, interpreter.display().to_string());
    assert_eq!(requests[2].args, pip_args);

    let baseline_args = vec![
        "-m".to_string(),
        "pip".to_string(),
        "install".to_string(),
        "--disable-pip-version-check".to_string(),
        "--no-input".to_string(),
        "--constraint".to_string(),
        constraints.display().to_string(),
        "jupyterlab==4.6.6".to_string(),
        "ipykernel>=6.28,<8".to_string(),
        EXTENSION_WHEEL.to_string(),
    ];
    assert_eq!(requests[3].program, interpreter.display().to_string());
    assert_eq!(requests[3].args, baseline_args);

    let course_args = vec![
        "-m".to_string(),
        "pip".to_string(),
        "install".to_string(),
        "--disable-pip-version-check".to_string(),
        "--no-input".to_string(),
        "--constraint".to_string(),
        constraints.display().to_string(),
        "--requirement".to_string(),
        package.join("pylock.toml").display().to_string(),
        "--no-deps".to_string(),
    ];
    assert_eq!(requests[4].program, interpreter.display().to_string());
    assert_eq!(requests[4].args, course_args);
    assert!(venv.join(STATE_DIR).join(MARKER_FILE).is_file());
}

#[test]
fn a_matching_fingerprint_reuses_the_environment_without_pip() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);
    manager(&runner, &package, &workspace).provision().unwrap();
    install_interpreter(&workspace);

    runner.clear();
    runner.enqueue(vec![success("Python 3.12.4\n")]);
    let environment = manager(&runner, &package, &workspace).provision().unwrap();

    assert_eq!(
        environment.interpreter,
        venv_interpreter(&course_environment_dir(&workspace))
    );
    assert_eq!(
        runner.requests(),
        vec![CommandRequest::new("python3", ["--version"], &workspace)]
    );
}

#[test]
fn a_course_input_change_reinstalls_only_course_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);
    manager(&runner, &package, &workspace).provision().unwrap();
    install_interpreter(&workspace);

    std::fs::write(
        package.join("pylock.toml"),
        "lock-version = \"1.0\"\ncreated-by = \"test\"\npackages = []\n# changed\n",
    )
    .unwrap();
    assert_reinstalls_only_course(&runner, &package, &workspace);

    std::fs::write(
        package.join("pyproject.toml"),
        "[project]\nname = \"acceptance\"\nversion = \"0.1.0\"\nrequires-python = \">=3.11\"\ndependencies = []\n",
    )
    .unwrap();
    assert_reinstalls_only_course(&runner, &package, &workspace);
}

fn assert_reinstalls_only_course(runner: &FakeRunner, package: &Path, workspace: &Path) {
    runner.clear();
    runner.enqueue(vec![success("Python 3.12.4\n"), success("")]);
    manager(runner, package, workspace).provision().unwrap();

    let requests = runner.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0],
        CommandRequest::new("python3", ["--version"], workspace)
    );
    assert!(requests[1].args.contains(&"--requirement".to_string()));
    assert!(
        requests[1]
            .args
            .contains(&package.join("pylock.toml").display().to_string())
    );
    assert!(requests[1].args.contains(&"--no-deps".to_string()));
    assert!(
        !requests[1]
            .args
            .iter()
            .any(|arg| arg.starts_with("jupyterlab=="))
    );
}

#[test]
fn a_rebuilt_extension_wheel_only_reinstalls_the_wheel() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let wheel = directory.path().join("wheels/extension.whl");
    std::fs::create_dir_all(wheel.parent().unwrap()).unwrap();
    std::fs::write(&wheel, "old wheel").unwrap();
    let baseline = CourseEnvironmentBaseline::new("0.2.4", &wheel);
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);
    RuntimeManager::new(&runner, &package, &workspace, baseline)
        .provision()
        .unwrap();
    install_interpreter(&workspace);

    std::fs::write(&wheel, "new wheel").unwrap();
    runner.clear();
    runner.enqueue(vec![success("Python 3.12.4\n"), success("")]);
    RuntimeManager::new(
        &runner,
        &package,
        &workspace,
        CourseEnvironmentBaseline::new("0.2.4", &wheel),
    )
    .provision()
    .unwrap();

    let requests = runner.requests();
    assert_eq!(requests.len(), 2);
    let args = &requests[1].args;
    assert!(args.contains(&"--force-reinstall".to_string()));
    assert!(args.contains(&"--no-deps".to_string()));
    assert!(args.contains(&wheel.display().to_string()));
    assert!(
        !requests
            .iter()
            .any(|request| request.args.get(1).is_some_and(|arg| arg == "venv"))
    );
}

#[test]
fn a_python_or_baseline_version_change_rebuilds_the_environment() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);
    manager(&runner, &package, &workspace).provision().unwrap();
    install_interpreter(&workspace);

    let mut changed = baseline();
    changed.jupyterlab_version = "4.6.7".to_string();
    runner.clear();
    runner.enqueue(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);
    RuntimeManager::new(&runner, &package, &workspace, changed.clone())
        .provision()
        .unwrap();
    let requests = runner.requests();
    assert_eq!(requests[1].args[1], "venv");
    assert_eq!(requests[1].args[2], "--clear");
    assert!(requests[3].args.contains(&"jupyterlab==4.6.7".to_string()));

    changed.extension_version = "0.2.5".to_string();
    runner.clear();
    runner.enqueue(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);
    RuntimeManager::new(&runner, &package, &workspace, changed.clone())
        .provision()
        .unwrap();
    let requests = runner.requests();
    assert_eq!(requests[1].args[2], "--clear");
    assert!(
        std::fs::read_to_string(workspace.join(".venv/.cynapse/constraints.txt"))
            .unwrap()
            .contains("cynapse-jupyterlab-extension==0.2.5")
    );

    runner.clear();
    runner.enqueue(vec![
        success("Python 3.13.1\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);
    RuntimeManager::new(&runner, &package, &workspace, changed)
        .provision()
        .unwrap();
    let requests = runner.requests();
    assert_eq!(requests[1].args[2], "--clear");
}

#[test]
fn a_corrupt_marker_rebuilds_the_environment() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let marker = course_environment_dir(&workspace)
        .join(STATE_DIR)
        .join(MARKER_FILE);
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "not json").unwrap();
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);

    manager(&runner, &package, &workspace).provision().unwrap();

    assert_eq!(runner.requests().len(), 5);
    assert!(
        std::fs::read_to_string(&marker)
            .unwrap()
            .contains("\"python\"")
    );
}

#[test]
fn a_course_without_pyproject_is_a_typed_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    std::fs::remove_file(package.join("pyproject.toml")).unwrap();
    let runner = FakeRunner::new(Vec::new());

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeSetupError::MissingPyproject { ref package_root }
                if package_root == &package
        ),
        "{error}"
    );
    assert!(runner.requests().is_empty());
}

#[test]
fn a_course_without_pylock_is_a_typed_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    std::fs::remove_file(package.join("pylock.toml")).unwrap();
    let runner = FakeRunner::new(Vec::new());

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeSetupError::MissingPylock { ref package_root }
                if package_root == &package
        ),
        "{error}"
    );
    assert!(runner.requests().is_empty());
}

#[test]
fn requirements_files_are_not_read() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    std::fs::write(package.join("requirements.lock"), "numpy==2.1.0\n").unwrap();
    std::fs::write(package.join("requirements.txt"), "numpy==2.2.0\n").unwrap();
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);

    manager(&runner, &package, &workspace).provision().unwrap();

    let requests = runner.requests();
    let course = &requests[4];
    assert!(
        course
            .args
            .contains(&package.join("pylock.toml").display().to_string())
    );
    assert!(
        !course
            .args
            .iter()
            .any(|arg| arg.ends_with("requirements.lock") || arg.ends_with("requirements.txt"))
    );
}

#[test]
fn venv_creation_failure_is_typed() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        failure("Error: [Errno 13] Permission denied"),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeSetupError::VenvCreation { ref message }
                if message.contains("Permission denied")
        ),
        "{error}"
    );
}

#[test]
fn a_baseline_constraint_conflict_is_typed() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        failure(
            "ERROR: Cannot install jupyterlab==4.6.6 and jupyterlab==5.0 because these package versions have conflicting dependencies.",
        ),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(error, RuntimeSetupError::ConstraintConflict { .. }),
        "{error}"
    );
}

#[test]
fn a_course_pip_failure_is_typed() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        failure("ERROR: Failed building wheel for numpy"),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeSetupError::PipFailed { ref message }
                if message.contains("Failed building wheel")
        ),
        "{error}"
    );
}

#[test]
fn offline_setup_is_typed_and_actionable() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        failure(
            "ERROR: Could not find a version that satisfies the requirement jupyterlab==4.6.6\n\
             ERROR: No matching distribution found for jupyterlab==4.6.6\n\
             WARNING: Retrying after connection broken by 'NameResolutionError': Temporary failure in name resolution",
        ),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(error, RuntimeSetupError::NetworkUnavailable { .. }),
        "{error}"
    );
}

#[test]
fn an_unreachable_index_wins_over_pips_resolution_impossible_wording() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        failure(
            "WARNING: Retrying after connection broken by 'NewConnectionError': Failed to establish a new connection: [WinError 10061]\n\
             ERROR: Cannot install jupyterlab==4.6.6 because these package versions have conflicting dependencies.\n\
             ERROR: ResolutionImpossible\n\
             The user requested (constraint) jupyterlab==4.6.6",
        ),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(error, RuntimeSetupError::NetworkUnavailable { .. }),
        "{error}"
    );
}

#[test]
fn a_missing_package_version_is_a_pip_failure_not_an_offline_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = course(directory.path());
    let runner = FakeRunner::new(vec![
        success("Python 3.12.4\n"),
        success(""),
        success(""),
        success(""),
        failure(
            "ERROR: Could not find a version that satisfies the requirement numpy==9.9.9 (from versions: 2.1.0)\n\
             ERROR: No matching distribution found for numpy==9.9.9",
        ),
    ]);

    let error = manager(&runner, &package, &workspace)
        .provision()
        .unwrap_err();

    assert!(
        matches!(error, RuntimeSetupError::PipFailed { .. }),
        "{error}"
    );
}
