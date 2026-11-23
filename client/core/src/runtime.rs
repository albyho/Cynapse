//! Runtime Manager: creates, fingerprints, and reuses the Course Environment.
//!
//! One environment lives at `<Course Workspace>/.venv` per course. The learner
//! installs Python; this module only detects a compatible interpreter, creates
//! the environment with it, installs the Cynapse baseline under constraints,
//! then installs the course lock under the same constraints. Every later
//! command uses the environment's interpreter.
//!
//! Process execution goes through [`CommandRunner`] so tests never spawn pip.
//! Provisioning is blocking; call it from a background executor, and it never
//! needs the JupyterLab server or an Adapter connection.

use cynapse_client_i18n::t_with;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::course::Course;

/// Directory of the Course Environment inside the Course Workspace.
pub const COURSE_ENVIRONMENT_DIR: &str = ".venv";

/// JupyterLab baseline pinned by the Client; the v1 contract supports 4.6.x.
pub const BASELINE_JUPYTERLAB_VERSION: &str = "4.6.6";

/// Distribution name of the Cynapse-owned prebuilt JupyterLab extension wheel.
pub const BASELINE_EXTENSION_PACKAGE: &str = "cynapse-jupyterlab-extension";

const BASELINE_IPYKERNEL: &str = "ipykernel>=6.28,<8";
const MINIMUM_PYTHON: PythonVersion = PythonVersion {
    major: 3,
    minor: 10,
};
const STATE_DIR: &str = ".cynapse";
const MARKER_FILE: &str = "environment.json";
const CONSTRAINTS_FILE: &str = "constraints.txt";
/// Bump when the provisioning contract changes so old markers rebuild.
const ENVIRONMENT_SCHEMA: u32 = 2;
const PYTHON_CANDIDATES: &[(&str, &[&str])] = &[("python3", &[]), ("python", &[]), ("py", &["-3"])];
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;

/// A learner Python version reduced to the parts the environment identity uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PythonVersion {
    pub major: u64,
    pub minor: u64,
}

impl std::fmt::Display for PythonVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

/// One external command the Runtime Manager wants to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl CommandRequest {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: cwd.into(),
        }
    }
}

/// Bounded result of one external command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Seam between the Runtime Manager and the operating system.
pub trait CommandRunner {
    fn run(&self, request: &CommandRequest) -> std::io::Result<CommandResult>;
}

/// Production runner: spawns the command and keeps a bounded tail of output.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, request: &CommandRequest) -> std::io::Result<CommandResult> {
        let mut child = std::process::Command::new(&request.program)
            .args(&request.args)
            .current_dir(&request.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .expect("stdout is piped for every Runtime Manager command");
        let stderr = child
            .stderr
            .take()
            .expect("stderr is piped for every Runtime Manager command");
        let stdout = std::thread::spawn(move || read_tail(stdout));
        let stderr = std::thread::spawn(move || read_tail(stderr));
        let status = child.wait()?;
        Ok(CommandResult {
            success: status.success(),
            stdout: join_tail(stdout)?,
            stderr: join_tail(stderr)?,
        })
    }
}

/// pip can emit unbounded output; keep only the tail needed for diagnostics so
/// a failing install never buffers the whole transcript.
fn read_tail(mut reader: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut tail = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        tail.extend_from_slice(&buffer[..read]);
        if tail.len() > MAX_COMMAND_OUTPUT_BYTES {
            tail.drain(..tail.len() - MAX_COMMAND_OUTPUT_BYTES);
        }
    }
    Ok(tail)
}

fn join_tail(handle: std::thread::JoinHandle<std::io::Result<Vec<u8>>>) -> std::io::Result<String> {
    let bytes = handle
        .join()
        .map_err(|_| std::io::Error::other("command output reader panicked"))??;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Client-owned baseline the course pylock must not replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseEnvironmentBaseline {
    pub jupyterlab_version: String,
    pub extension_package: String,
    pub extension_version: String,
    pub extension_wheel: PathBuf,
}

impl CourseEnvironmentBaseline {
    pub fn new(extension_version: impl Into<String>, extension_wheel: impl Into<PathBuf>) -> Self {
        Self {
            jupyterlab_version: BASELINE_JUPYTERLAB_VERSION.to_string(),
            extension_package: BASELINE_EXTENSION_PACKAGE.to_string(),
            extension_version: extension_version.into(),
            extension_wheel: extension_wheel.into(),
        }
    }
}

/// A provisioned environment, ready for JupyterLab and Checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseEnvironment {
    pub interpreter: PathBuf,
    pub python: PythonVersion,
}

/// Typed provisioning failures. Each one is actionable on its own.
#[derive(Debug)]
pub enum RuntimeSetupError {
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    PythonNotFound {
        required: PythonVersion,
    },
    PythonIncompatible {
        found: PythonVersion,
        required: PythonVersion,
    },
    VenvCreation {
        message: String,
    },
    PipFailed {
        message: String,
    },
    ConstraintConflict {
        message: String,
    },
    NetworkUnavailable {
        message: String,
    },
    MissingPyproject {
        package_root: PathBuf,
    },
    MissingPylock {
        package_root: PathBuf,
    },
}

impl std::fmt::Display for RuntimeSetupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Io { path, error } => t_with(
                "error.runtime_io",
                &[
                    ("path", &path.display().to_string()),
                    ("error", &error.to_string()),
                ],
            ),
            Self::PythonNotFound { required } => t_with(
                "error.runtime_python_missing",
                &[("required", &required.to_string())],
            ),
            Self::PythonIncompatible { found, required } => t_with(
                "error.runtime_python_incompatible",
                &[
                    ("found", &found.to_string()),
                    ("required", &required.to_string()),
                ],
            ),
            Self::VenvCreation { message } => {
                t_with("error.runtime_venv_failed", &[("error", message)])
            }
            Self::PipFailed { message } => {
                t_with("error.runtime_pip_failed", &[("error", message)])
            }
            Self::ConstraintConflict { message } => {
                t_with("error.runtime_constraint_conflict", &[("error", message)])
            }
            Self::NetworkUnavailable { message } => {
                t_with("error.runtime_offline", &[("error", message)])
            }
            Self::MissingPyproject { package_root } => t_with(
                "error.runtime_missing_pyproject",
                &[("path", &package_root.display().to_string())],
            ),
            Self::MissingPylock { package_root } => t_with(
                "error.runtime_missing_pylock",
                &[("path", &package_root.display().to_string())],
            ),
        };
        formatter.write_str(&message)
    }
}

impl std::error::Error for RuntimeSetupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Creates and reuses one course's environment.
pub struct RuntimeManager<'a> {
    runner: &'a dyn CommandRunner,
    package_root: PathBuf,
    workspace: PathBuf,
    baseline: CourseEnvironmentBaseline,
}

impl<'a> RuntimeManager<'a> {
    pub fn new(
        runner: &'a dyn CommandRunner,
        package_root: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
        baseline: CourseEnvironmentBaseline,
    ) -> Self {
        Self {
            runner,
            package_root: package_root.into(),
            workspace: workspace.into(),
            baseline,
        }
    }

    /// Provision an open course: the package root supplies `pyproject.toml`
    /// and `pylock.toml`, and the Course Workspace hosts `.venv`.
    pub fn for_course(
        runner: &'a dyn CommandRunner,
        course: &Course,
        baseline: CourseEnvironmentBaseline,
    ) -> Self {
        Self::new(
            runner,
            &course.package_root,
            course.workspace_path(),
            baseline,
        )
    }

    /// Creates or reuses the environment and returns its interpreter.
    pub fn provision(&self) -> Result<CourseEnvironment, RuntimeSetupError> {
        let pyproject_path = self.package_root.join("pyproject.toml");
        let pyproject = match std::fs::read_to_string(&pyproject_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(RuntimeSetupError::MissingPyproject {
                    package_root: self.package_root.clone(),
                });
            }
            Err(error) => {
                return Err(RuntimeSetupError::Io {
                    path: pyproject_path,
                    error,
                });
            }
        };
        let required = required_python(&pyproject);
        let pylock_path = self.package_root.join("pylock.toml");
        let pylock = match std::fs::read_to_string(&pylock_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(RuntimeSetupError::MissingPylock {
                    package_root: self.package_root.clone(),
                });
            }
            Err(error) => {
                return Err(RuntimeSetupError::Io {
                    path: pylock_path,
                    error,
                });
            }
        };
        let (learner_python, python) = self.detect_python(required)?;
        let extension_wheel = std::fs::read(&self.baseline.extension_wheel)
            .map(|contents| content_hash(&contents))
            .unwrap_or_default();
        let fingerprint = EnvironmentFingerprint {
            schema: ENVIRONMENT_SCHEMA,
            python,
            jupyterlab: self.baseline.jupyterlab_version.clone(),
            extension: self.baseline.extension_version.clone(),
            extension_wheel,
            pyproject: content_hash(pyproject.as_bytes()),
            pylock: content_hash(pylock.as_bytes()),
        };
        let venv = course_environment_dir(&self.workspace);
        let interpreter = venv_interpreter(&venv);
        let stored = read_fingerprint(&venv);
        if stored.as_ref() == Some(&fingerprint) && interpreter.is_file() {
            return Ok(CourseEnvironment {
                interpreter,
                python,
            });
        }

        // Python or JupyterLab baseline changes need a clean environment. A
        // rebuilt extension wheel only needs a force-reinstall of that wheel;
        // recreating the venv would reinstall the entire JupyterLab baseline
        // and course lock for an Adapter-only change.
        let rebuild_baseline = stored.as_ref().is_none_or(|stored| {
            stored.schema != fingerprint.schema
                || stored.python != fingerprint.python
                || stored.jupyterlab != fingerprint.jupyterlab
                || stored.extension != fingerprint.extension
        }) || !interpreter.is_file();
        let reinstall_wheel = stored
            .as_ref()
            .is_some_and(|stored| stored.extension_wheel != fingerprint.extension_wheel);
        let reinstall_course = rebuild_baseline
            || stored.as_ref().is_none_or(|stored| {
                stored.pyproject != fingerprint.pyproject || stored.pylock != fingerprint.pylock
            });
        if rebuild_baseline {
            self.create_environment(&learner_python, &venv)?;
            self.install_pip(&interpreter, &venv)?;
            self.install_baseline(&interpreter, &venv)?;
        } else if reinstall_wheel {
            self.install_extension(&interpreter, &venv)?;
        }
        if reinstall_course {
            self.install_course(&interpreter, &venv, &pylock_path)?;
        }
        write_fingerprint(&venv, &fingerprint)?;
        Ok(CourseEnvironment {
            interpreter,
            python,
        })
    }

    fn create_environment(
        &self,
        learner_python: &str,
        venv: &Path,
    ) -> Result<(), RuntimeSetupError> {
        let mut args = vec!["-m".to_string(), "venv".to_string()];
        if venv.exists() {
            args.push("--clear".to_string());
        }
        args.push(venv.display().to_string());
        let result = self.run(&CommandRequest::new(learner_python, args, &self.workspace))?;
        if !result.success {
            return Err(RuntimeSetupError::VenvCreation {
                message: failure_message(&result),
            });
        }
        Ok(())
    }

    /// `pylock.toml` needs pip 26.1 or newer; keep the requirement a minimum
    /// so a venv that already satisfies it is not upgraded.
    fn install_pip(&self, interpreter: &Path, venv: &Path) -> Result<(), RuntimeSetupError> {
        let specs = vec!["pip>=26.1".to_string()];
        self.run_pip(interpreter, venv, &specs)
    }

    fn install_baseline(&self, interpreter: &Path, venv: &Path) -> Result<(), RuntimeSetupError> {
        let specs = vec![
            format!("jupyterlab=={}", self.baseline.jupyterlab_version),
            BASELINE_IPYKERNEL.to_string(),
            self.baseline.extension_wheel.display().to_string(),
        ];
        self.run_pip(interpreter, venv, &specs)
    }

    /// A rebuilt prebuilt extension replaces only its own files. The baseline
    /// and course dependencies are already installed in a matching venv.
    fn install_extension(&self, interpreter: &Path, venv: &Path) -> Result<(), RuntimeSetupError> {
        let specs = vec![
            "--force-reinstall".to_string(),
            "--no-deps".to_string(),
            self.baseline.extension_wheel.display().to_string(),
        ];
        self.run_pip(interpreter, venv, &specs)
    }

    /// Installs the exact dependency set from `pylock.toml`. `--no-deps` keeps
    /// pip from resolving packages that are not in the lock.
    fn install_course(
        &self,
        interpreter: &Path,
        venv: &Path,
        pylock_path: &Path,
    ) -> Result<(), RuntimeSetupError> {
        let specs = vec![
            "--requirement".to_string(),
            pylock_path.display().to_string(),
            "--no-deps".to_string(),
        ];
        self.run_pip(interpreter, venv, &specs)
    }

    fn run_pip(
        &self,
        interpreter: &Path,
        venv: &Path,
        specs: &[String],
    ) -> Result<(), RuntimeSetupError> {
        let constraints = self.write_constraints(venv)?;
        let mut args = vec![
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "--disable-pip-version-check".to_string(),
            "--no-input".to_string(),
            "--constraint".to_string(),
            constraints.display().to_string(),
        ];
        args.extend(specs.iter().cloned());
        let result = self.run(&CommandRequest::new(
            interpreter.display().to_string(),
            args,
            &self.workspace,
        ))?;
        if !result.success {
            return Err(self.pip_failure(&result));
        }
        Ok(())
    }

    /// pip reports every resolution failure as non-zero exit; only its text
    /// tells a baseline constraint conflict from a network or build failure.
    /// An unreachable index also ends in `ResolutionImpossible`, so connection
    /// failures win before conflict wording is considered.
    fn pip_failure(&self, result: &CommandResult) -> RuntimeSetupError {
        let message = failure_message(result);
        let lowered = message.to_ascii_lowercase();
        let network_failure = lowered.contains("newconnectionerror")
            || lowered.contains("failed to establish a new connection")
            || lowered.contains("nameresolutionerror")
            || lowered.contains("temporary failure in name resolution")
            || lowered.contains("name resolution")
            || lowered.contains("proxyerror")
            || lowered.contains("connection refused")
            || lowered.contains("connection broken")
            || lowered.contains("getaddrinfo")
            || lowered.contains("network is unreachable");
        if network_failure {
            return RuntimeSetupError::NetworkUnavailable { message };
        }
        let names_baseline = lowered.contains("jupyterlab")
            || lowered.contains("ipykernel")
            || lowered.contains(&self.baseline.extension_package.to_ascii_lowercase());
        let conflicts = lowered.contains("conflicting dependencies")
            || lowered.contains("resolutionimpossible")
            || lowered.contains("cannot install");
        if conflicts && (names_baseline || lowered.contains("constraint")) {
            RuntimeSetupError::ConstraintConflict { message }
        } else {
            RuntimeSetupError::PipFailed { message }
        }
    }

    fn write_constraints(&self, venv: &Path) -> Result<PathBuf, RuntimeSetupError> {
        let path = venv.join(STATE_DIR).join(CONSTRAINTS_FILE);
        let contents = format!(
            "jupyterlab=={}\n{}\n{}=={}\n",
            self.baseline.jupyterlab_version,
            BASELINE_IPYKERNEL,
            self.baseline.extension_package,
            self.baseline.extension_version
        );
        write_text(&path, &contents)?;
        Ok(path)
    }

    fn run(&self, request: &CommandRequest) -> Result<CommandResult, RuntimeSetupError> {
        self.runner
            .run(request)
            .map_err(|error| RuntimeSetupError::Io {
                path: PathBuf::from(&request.program),
                error,
            })
    }

    fn detect_python(
        &self,
        required: PythonVersion,
    ) -> Result<(String, PythonVersion), RuntimeSetupError> {
        let mut first_found = None;
        for (program, prefix) in PYTHON_CANDIDATES {
            let mut args = prefix.to_vec();
            args.push("--version");
            let request = CommandRequest::new(*program, args.iter().copied(), &self.workspace);
            let Ok(result) = self.runner.run(&request) else {
                continue;
            };
            if !result.success {
                continue;
            }
            let version = parse_python_version(&result.stdout)
                .or_else(|| parse_python_version(&result.stderr));
            let Some(version) = version else {
                continue;
            };
            if version >= required {
                return Ok((program.to_string(), version));
            }
            first_found.get_or_insert(version);
        }
        match first_found {
            Some(found) => Err(RuntimeSetupError::PythonIncompatible { found, required }),
            None => Err(RuntimeSetupError::PythonNotFound { required }),
        }
    }
}

pub fn course_environment_dir(workspace: &Path) -> PathBuf {
    workspace.join(COURSE_ENVIRONMENT_DIR)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EnvironmentFingerprint {
    schema: u32,
    python: PythonVersion,
    jupyterlab: String,
    extension: String,
    extension_wheel: String,
    pyproject: String,
    pylock: String,
}

fn marker_path(venv: &Path) -> PathBuf {
    venv.join(STATE_DIR).join(MARKER_FILE)
}

fn read_fingerprint(venv: &Path) -> Option<EnvironmentFingerprint> {
    let contents = std::fs::read_to_string(marker_path(venv)).ok()?;
    serde_json::from_str(&contents).ok()
}

fn write_fingerprint(
    venv: &Path,
    fingerprint: &EnvironmentFingerprint,
) -> Result<(), RuntimeSetupError> {
    let path = marker_path(venv);
    let contents =
        serde_json::to_vec_pretty(fingerprint).expect("course environment fingerprint serializes");
    crate::atomic_file::write_atomic(&path, &contents)
        .map_err(|error| RuntimeSetupError::Io { path, error })
}

fn write_text(path: &Path, contents: &str) -> Result<(), RuntimeSetupError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| RuntimeSetupError::Io {
            path: path.to_path_buf(),
            error,
        })?;
    }
    std::fs::write(path, contents).map_err(|error| RuntimeSetupError::Io {
        path: path.to_path_buf(),
        error,
    })
}

/// shortcut: 64-bit FNV-1a identifies file content; collisions are negligible
/// for change detection. Upgrade to SHA-256 if this ever becomes a trust check.
fn content_hash(contents: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in contents {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn failure_message(result: &CommandResult) -> String {
    format!("{}\n{}", result.stderr.trim(), result.stdout.trim())
        .trim()
        .to_string()
}

fn venv_interpreter(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// Only the `>=X.Y` lower bound of `requires-python` is enforced.
/// shortcut: PEP 440 upper bounds and exclusions are ignored, upgrade when a
/// course needs to forbid a Python major version.
fn required_python(pyproject: &str) -> PythonVersion {
    let declared = pyproject.lines().find_map(|line| {
        let value = line.trim().strip_prefix("requires-python")?;
        let value = value.trim_start().strip_prefix('=')?.trim();
        let value = value.trim_matches(|character| character == '"' || character == '\'');
        let minimum = value.split(',').next()?;
        parse_major_minor(minimum.trim().strip_prefix(">=")?)
    });
    declared
        .map(|version| version.max(MINIMUM_PYTHON))
        .unwrap_or(MINIMUM_PYTHON)
}

fn parse_major_minor(value: &str) -> Option<PythonVersion> {
    let mut parts = value.trim().trim_start_matches('v').split('.');
    Some(PythonVersion {
        major: parts.next()?.parse().ok()?,
        minor: parts.next()?.parse().ok()?,
    })
}

fn parse_python_version(output: &str) -> Option<PythonVersion> {
    output.lines().find_map(|line| {
        let version = line.trim().strip_prefix("Python ")?;
        parse_major_minor(version.split_whitespace().next()?)
    })
}

#[cfg(test)]
mod tests;
