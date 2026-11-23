//! Manual acceptance for the Runtime Manager: a real venv and real pip.
//!
//! Run explicitly outside CI:
//! `CYNAPSE_EXTENSION_WHEEL=<wheel> cargo test -p cynapse-client-core --test runtime_acceptance -- --ignored --nocapture`
//!
//! Until ticket #143 ships the Cynapse extension wheel, a PyPI requirement may
//! stand in for it, for example:
//! `CYNAPSE_EXTENSION_WHEEL=packaging==24.2 CYNAPSE_EXTENSION_PACKAGE=packaging CYNAPSE_EXTENSION_VERSION=24.2`
//!
//! The baseline-conflict test generates a lock with `pip lock`, which needs
//! pip 26.1+ on the `PYTHON` interpreter (default: `python`).

use cynapse_client_core::{
    BASELINE_EXTENSION_PACKAGE, BASELINE_JUPYTERLAB_VERSION, CommandRequest, CommandRunner,
    CourseEnvironmentBaseline, RuntimeManager, RuntimeSetupError, SystemCommandRunner,
    course_environment_dir,
};

fn acceptance_package(directory: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let package = directory.join("course");
    let workspace = package.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
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
    (package, workspace)
}

fn write_requirement_lock(package: &std::path::Path, requirement: &str) {
    let python = std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string());
    let result = SystemCommandRunner
        .run(&CommandRequest::new(
            python,
            [
                "-m".to_string(),
                "pip".to_string(),
                "lock".to_string(),
                requirement.to_string(),
                "-o".to_string(),
                package.join("pylock.toml").display().to_string(),
            ],
            package,
        ))
        .unwrap();
    assert!(
        result.success,
        "pip lock failed; it needs pip 26.1 or newer: {}",
        result.stderr
    );
}

fn acceptance_baseline() -> CourseEnvironmentBaseline {
    let wheel = std::env::var_os("CYNAPSE_EXTENSION_WHEEL")
        .expect("set CYNAPSE_EXTENSION_WHEEL to the Cynapse JupyterLab extension wheel");
    CourseEnvironmentBaseline {
        jupyterlab_version: BASELINE_JUPYTERLAB_VERSION.to_string(),
        extension_package: std::env::var("CYNAPSE_EXTENSION_PACKAGE")
            .unwrap_or_else(|_| BASELINE_EXTENSION_PACKAGE.to_string()),
        extension_version: std::env::var("CYNAPSE_EXTENSION_VERSION")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
        extension_wheel: wheel.into(),
    }
}

#[test]
#[ignore = "real venv and pip install; run manually"]
fn provisions_and_reuses_a_real_course_environment() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = acceptance_package(directory.path());
    let baseline = acceptance_baseline();
    let runner = SystemCommandRunner;

    let first = RuntimeManager::new(&runner, &package, &workspace, baseline.clone())
        .provision()
        .expect("first provision installs JupyterLab, the baseline, and the course lock");
    println!(
        "interpreter: {} (Python {})",
        first.interpreter.display(),
        first.python
    );
    let imported = runner
        .run(&CommandRequest::new(
            first.interpreter.display().to_string(),
            ["-c", "import jupyterlab, ipykernel"],
            &workspace,
        ))
        .unwrap();
    assert!(imported.success, "{}", imported.stderr);

    let marker = course_environment_dir(&workspace).join(".cynapse/environment.json");
    let modified = std::fs::metadata(&marker).unwrap().modified().unwrap();
    let second = RuntimeManager::new(&runner, &package, &workspace, baseline)
        .provision()
        .expect("second provision reuses the environment");
    assert_eq!(first, second);
    assert_eq!(
        std::fs::metadata(&marker).unwrap().modified().unwrap(),
        modified,
        "reuse must not rewrite the fingerprint marker"
    );
}

#[test]
#[ignore = "real venv and pip install; run manually"]
fn a_course_pylock_that_replaces_the_baseline_is_a_constraint_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = acceptance_package(directory.path());
    write_requirement_lock(&package, "jupyterlab==4.6.5");
    let runner = SystemCommandRunner;

    let error = RuntimeManager::new(&runner, &package, &workspace, acceptance_baseline())
        .provision()
        .unwrap_err();

    assert!(
        matches!(error, RuntimeSetupError::ConstraintConflict { .. }),
        "{error}"
    );
}

#[test]
#[ignore = "set PIP_INDEX_URL to an unreachable index; run manually"]
fn offline_setup_reports_network_unavailable() {
    assert!(
        std::env::var_os("PIP_INDEX_URL").is_some(),
        "run with PIP_INDEX_URL set to an unreachable index, for example http://127.0.0.1:1"
    );
    let directory = tempfile::tempdir().unwrap();
    let (package, workspace) = acceptance_package(directory.path());
    let runner = SystemCommandRunner;

    let error = RuntimeManager::new(&runner, &package, &workspace, acceptance_baseline())
        .provision()
        .unwrap_err();

    assert!(
        matches!(error, RuntimeSetupError::NetworkUnavailable { .. }),
        "{error}"
    );
}
