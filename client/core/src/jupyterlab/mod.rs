//! JupyterLab server lifecycle: origin/port/token for one Active Course.
//!
//! The Client provisions the Course Environment (`.venv`), launches JupyterLab
//! on loopback with a per-launch token, and opens the default browser at
//! `/lab?token=…`. The browser page's Cynapse extension connects back on the
//! fixed control port; the Client accepts it by JupyterLab `Origin` rather than
//! a URL token. That handshake lives in [`crate::server`].
//!
//! Process spawning, readiness probing, and stale-server cleanup are traits so
//! tests never start a real JupyterLab. The stale marker is separate from the
//! Protocol discovery file `runtime.json` because a browser cannot read it.
//!
//! Submodules:
//! - `tests` — lifecycle, launch, stale cleanup, readiness, and URL decisions

use cynapse_client_i18n::t_with;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Fixed loopback port the JupyterLab extension calls back on.
pub const DEFAULT_CONTROL_PORT: u16 = 37654;

/// Stale-server marker in the Client runtime data directory. It is not
/// `runtime.json`; the Protocol discovery file keeps its own contract.
pub const STALE_SERVER_MARKER_FILE: &str = "jupyterlab-server.json";

const READINESS_POLL: Duration = Duration::from_millis(100);
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Where the current JupyterLab server is, so a later Client start can stop it
/// after an unclean exit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleServerMarker {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub workspace: PathBuf,
}

impl StaleServerMarker {
    pub fn path(runtime_dir: &Path) -> PathBuf {
        runtime_dir.join(STALE_SERVER_MARKER_FILE)
    }

    pub fn read(path: &Path) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }
}

/// One external JupyterLab command to spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JupyterLabLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Jupyter terminals inherit this environment, so Checks run with the
    /// Course Environment interpreter without a per-command wrapper.
    pub env: Vec<(OsString, OsString)>,
}

/// The spawned JupyterLab server process.
pub trait JupyterLabProcess: Send {
    fn pid(&self) -> u32;
    /// Non-blocking; true once the process has exited (successfully or not).
    fn has_exited(&mut self) -> bool;
    fn kill(&mut self) -> std::io::Result<()>;
}

pub trait JupyterLabSpawner: Send + Sync {
    fn spawn(&self, launch: &JupyterLabLaunch) -> std::io::Result<Box<dyn JupyterLabProcess>>;
}

/// Loopback readiness probe; the production implementation only checks TCP.
pub trait JupyterLabReadinessProbe: Send + Sync {
    fn is_ready(&self, port: u16) -> bool;
}

/// Stale-server detection and termination against the recorded marker.
pub trait StaleServerCleanup: Send + Sync {
    fn is_live(&self, marker: &StaleServerMarker) -> bool;
    fn terminate(&self, marker: &StaleServerMarker) -> std::io::Result<()>;
}

#[derive(Debug, Clone)]
pub struct JupyterLabServerConfig {
    pub interpreter: PathBuf,
    pub workspace: PathBuf,
    /// `0` asks the OS for a free loopback port.
    pub server_port: u16,
    pub token: String,
    pub marker_path: PathBuf,
    pub startup_timeout: Duration,
}

impl JupyterLabServerConfig {
    /// Browser handshake: only JupyterLab auth belongs in the URL. The control
    /// channel uses the fixed port and validates the browser `Origin`.
    pub fn browser_url(&self, server_port: u16) -> String {
        format!("http://127.0.0.1:{server_port}/lab?token={}", self.token)
    }

    fn launch(&self, server_port: u16) -> JupyterLabLaunch {
        let mut env = course_environment_launch_env(&self.interpreter);
        if let Some(settings) = jupyterlab_settings_dir(&self.interpreter) {
            env.push((
                OsString::from("JUPYTERLAB_SETTINGS_DIR"),
                settings.into_os_string(),
            ));
        }
        JupyterLabLaunch {
            program: self.interpreter.clone(),
            args: vec![
                "-m".to_string(),
                "jupyterlab".to_string(),
                "--no-browser".to_string(),
                "--ServerApp.ip=127.0.0.1".to_string(),
                format!("--ServerApp.port={server_port}"),
                "--ServerApp.port_retries=0".to_string(),
                "--MappingKernelManager.default_kernel_name=python3".to_string(),
                format!("--ServerApp.root_dir={}", self.workspace.display()),
                format!("--ServerApp.token={}", self.token),
            ],
            cwd: self.workspace.clone(),
            env,
        }
    }
}

fn jupyterlab_settings_dir(interpreter: &Path) -> Option<PathBuf> {
    let environment_root = interpreter.parent()?.parent()?;
    Some(
        environment_root
            .join("share")
            .join("jupyter")
            .join("lab")
            .join("user-settings"),
    )
}

fn ensure_jupyterlab_tracker_settings(
    config: &JupyterLabServerConfig,
) -> Result<(), JupyterLabStartError> {
    let settings = jupyterlab_settings_dir(&config.interpreter).ok_or_else(|| {
        JupyterLabStartError::State {
            path: config.interpreter.clone(),
            error: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "JupyterLab interpreter has no Course Environment root",
            ),
        }
    })?;
    // JupyterLab only reads user settings from this extension, not `tracker.json`
    // (see jupyterlab_server.settings_utils.SETTINGS_EXTENSION).
    let tracker = settings
        .join("@jupyterlab")
        .join("notebook-extension")
        .join("tracker.jupyterlab-settings");
    std::fs::create_dir_all(tracker.parent().expect("tracker has a parent")).map_err(|error| {
        JupyterLabStartError::State {
            path: tracker.clone(),
            error,
        }
    })?;
    let mut value = match std::fs::read_to_string(&tracker) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| JupyterLabStartError::State {
            path: tracker.clone(),
            error: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => {
            return Err(JupyterLabStartError::State {
                path: tracker,
                error,
            });
        }
    };
    let object = value
        .as_object_mut()
        .ok_or_else(|| JupyterLabStartError::State {
            path: tracker.clone(),
            error: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "JupyterLab tracker settings must be a JSON object",
            ),
        })?;
    if object.get("autoStartDefaultKernel") == Some(&serde_json::Value::Bool(true)) {
        return Ok(());
    }
    object.insert(
        "autoStartDefaultKernel".to_string(),
        serde_json::Value::Bool(true),
    );
    let text =
        serde_json::to_string_pretty(&value).map_err(|error| JupyterLabStartError::State {
            path: tracker.clone(),
            error: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        })?;
    std::fs::write(&tracker, format!("{text}\n")).map_err(|error| JupyterLabStartError::State {
        path: tracker,
        error,
    })
}

/// The server and every terminal it starts use the Course Environment. The
/// `Scripts` directory (or `bin` on Unix) goes first on PATH and `VIRTUAL_ENV`
/// makes `python` resolve to the course interpreter.
fn course_environment_launch_env(interpreter: &Path) -> Vec<(OsString, OsString)> {
    let scripts = interpreter.parent().unwrap_or_else(|| Path::new(""));
    let environment_root = scripts.parent().unwrap_or_else(|| Path::new(""));
    let mut paths = vec![scripts.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(paths).unwrap_or_else(|_| scripts.as_os_str().to_os_string());
    vec![
        (OsString::from("PATH"), path),
        (
            OsString::from("VIRTUAL_ENV"),
            environment_root.as_os_str().to_os_string(),
        ),
    ]
}

#[derive(Debug)]
pub enum JupyterLabStartError {
    PortInUse {
        port: u16,
    },
    Spawn {
        message: String,
    },
    NotReady {
        port: u16,
        seconds: u64,
    },
    Exited,
    State {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl std::fmt::Display for JupyterLabStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::PortInUse { port } => t_with(
                "error.jupyterlab_control_port_in_use",
                &[("port", &port.to_string())],
            ),
            Self::Spawn { message } => {
                t_with("error.jupyterlab_spawn_failed", &[("error", message)])
            }
            Self::NotReady { port, seconds } => t_with(
                "error.jupyterlab_server_not_ready",
                &[
                    ("port", &port.to_string()),
                    ("seconds", &seconds.to_string()),
                ],
            ),
            Self::Exited => t_with("error.jupyterlab_server_exited", &[]),
            Self::State { path, error } => t_with(
                "error.jupyterlab_state_failed",
                &[
                    ("path", &path.display().to_string()),
                    ("error", &error.to_string()),
                ],
            ),
        };
        formatter.write_str(&message)
    }
}

impl std::error::Error for JupyterLabStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::State { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// The API the Client model keeps for one Active Course.
pub trait JupyterLabRuntime: Send {
    fn workspace(&self) -> &Path;
    fn browser_url(&self) -> String;
    /// The loopback HTTP port; its origin is the control-channel proof.
    fn server_port(&self) -> u16;
    fn is_running(&mut self) -> bool;
    fn stop(&mut self);
}

/// Production seam: provision the Course Environment, then launch JupyterLab.
pub trait JupyterLabProvisioner: Send + Sync {
    fn start(&self, course: &crate::course::Course) -> Result<Box<dyn JupyterLabRuntime>, String>;

    /// Terminate a server left behind by an unclean Client exit. Runs at
    /// Client startup; the marker is separate from the Protocol discovery file.
    fn cleanup_stale(&self) {}
}

/// One running JupyterLab server plus its stale marker.
pub struct JupyterLabServer {
    config: JupyterLabServerConfig,
    port: u16,
    process: Box<dyn JupyterLabProcess>,
}

impl JupyterLabServer {
    /// Starts a fresh server after terminating a confirmed stale one.
    pub fn start(
        config: JupyterLabServerConfig,
        spawner: &dyn JupyterLabSpawner,
        probe: &dyn JupyterLabReadinessProbe,
        stale: &dyn StaleServerCleanup,
    ) -> Result<Self, JupyterLabStartError> {
        cleanup_stale(&config.marker_path, stale);
        ensure_jupyterlab_tracker_settings(&config)?;
        let port = available_port(config.server_port)?;
        let mut process =
            spawner
                .spawn(&config.launch(port))
                .map_err(|error| JupyterLabStartError::Spawn {
                    message: error.to_string(),
                })?;
        let marker = StaleServerMarker {
            pid: process.pid(),
            port,
            token: config.token.clone(),
            workspace: config.workspace.clone(),
        };
        if let Err(error) = write_marker(&config.marker_path, &marker) {
            let _ = process.kill();
            return Err(JupyterLabStartError::State {
                path: config.marker_path.clone(),
                error,
            });
        }
        match wait_until_ready(&config, port, probe, process.as_mut()) {
            Readiness::Ready => {}
            Readiness::Exited => {
                let _ = std::fs::remove_file(&config.marker_path);
                return Err(JupyterLabStartError::Exited);
            }
            Readiness::TimedOut => {
                let _ = process.kill();
                let _ = std::fs::remove_file(&config.marker_path);
                return Err(JupyterLabStartError::NotReady {
                    port,
                    seconds: config.startup_timeout.as_secs(),
                });
            }
        }
        Ok(Self {
            config,
            port,
            process,
        })
    }

    pub fn browser_url(&self) -> String {
        self.config.browser_url(self.port)
    }

    pub fn server_port(&self) -> u16 {
        self.port
    }

    pub fn workspace(&self) -> &Path {
        &self.config.workspace
    }

    pub fn is_running(&mut self) -> bool {
        !self.process.has_exited()
    }

    pub fn stop(&mut self) {
        if !self.process.has_exited() {
            let _ = self.process.kill();
        }
        let _ = std::fs::remove_file(&self.config.marker_path);
    }
}

impl JupyterLabRuntime for JupyterLabServer {
    fn workspace(&self) -> &Path {
        JupyterLabServer::workspace(self)
    }

    fn browser_url(&self) -> String {
        JupyterLabServer::browser_url(self)
    }

    fn server_port(&self) -> u16 {
        JupyterLabServer::server_port(self)
    }

    fn is_running(&mut self) -> bool {
        JupyterLabServer::is_running(self)
    }

    fn stop(&mut self) {
        JupyterLabServer::stop(self);
    }
}

impl Drop for JupyterLabServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn cleanup_stale(marker_path: &Path, stale: &dyn StaleServerCleanup) {
    let Some(marker) = StaleServerMarker::read(marker_path) else {
        let _ = std::fs::remove_file(marker_path);
        return;
    };
    if stale.is_live(&marker) {
        log::info!(
            target: "cynapse.jupyterlab",
            "stale_server.terminating pid={} port={}",
            marker.pid,
            marker.port
        );
        let _ = stale.terminate(&marker);
    }
    let _ = std::fs::remove_file(marker_path);
}

fn write_marker(path: &Path, marker: &StaleServerMarker) -> std::io::Result<()> {
    let contents = serde_json::to_vec_pretty(marker).expect("stale server marker serializes");
    crate::atomic_file::write_atomic(path, &contents)
}

fn available_port(requested: u16) -> Result<u16, JupyterLabStartError> {
    let listener = TcpListener::bind(("127.0.0.1", requested)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            JupyterLabStartError::PortInUse { port: requested }
        } else {
            JupyterLabStartError::Spawn {
                message: error.to_string(),
            }
        }
    })?;
    let port = listener
        .local_addr()
        .map_err(|error| JupyterLabStartError::Spawn {
            message: error.to_string(),
        })?
        .port();
    drop(listener);
    Ok(port)
}

enum Readiness {
    Ready,
    Exited,
    TimedOut,
}

/// Ports 0 and explicit ports both go through the same readiness loop.
fn wait_until_ready(
    config: &JupyterLabServerConfig,
    port: u16,
    probe: &dyn JupyterLabReadinessProbe,
    process: &mut dyn JupyterLabProcess,
) -> Readiness {
    let deadline = Instant::now() + config.startup_timeout;
    loop {
        if probe.is_ready(port) {
            return Readiness::Ready;
        }
        if process.has_exited() {
            return Readiness::Exited;
        }
        if Instant::now() >= deadline {
            return Readiness::TimedOut;
        }
        std::thread::sleep(READINESS_POLL);
    }
}

/// Production process spawner. JupyterLab logs go to the null device so a full
/// pipe can never block the server; readiness is the TCP probe.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemJupyterLabSpawner;

impl JupyterLabSpawner for SystemJupyterLabSpawner {
    fn spawn(&self, launch: &JupyterLabLaunch) -> std::io::Result<Box<dyn JupyterLabProcess>> {
        let mut command = std::process::Command::new(&launch.program);
        command
            .args(&launch.args)
            .current_dir(&launch.cwd)
            .envs(launch.env.iter().map(|(key, value)| (key, value)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = command.spawn()?;
        Ok(Box::new(SystemJupyterLabProcess { child }))
    }
}

struct SystemJupyterLabProcess {
    child: std::process::Child,
}

impl JupyterLabProcess for SystemJupyterLabProcess {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn has_exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    fn kill(&mut self) -> std::io::Result<()> {
        // Kill the process tree so a course switch does not orphan kernels.
        let _ = terminate_process(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

/// TCP readiness is enough: JupyterLab binds its HTTP port only after the app
/// finishes starting, and the extension handshake has its own bounded wait.
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpJupyterLabProbe;

impl JupyterLabReadinessProbe for TcpJupyterLabProbe {
    fn is_ready(&self, port: u16) -> bool {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok()
    }
}

/// A marker is stale only when its JupyterLab answers with the recorded token;
/// that keeps a recycled PID from terminating an unrelated process.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemStaleServerCleanup;

impl StaleServerCleanup for SystemStaleServerCleanup {
    fn is_live(&self, marker: &StaleServerMarker) -> bool {
        let address = SocketAddr::from(([127, 0, 0, 1], marker.port));
        let Ok(mut stream) = TcpStream::connect_timeout(&address, PROBE_TIMEOUT) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
        let _ = stream.set_write_timeout(Some(PROBE_TIMEOUT));
        let request = format!(
            "GET /lab?token={} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            marker.token, marker.port
        );
        if stream.write_all(request.as_bytes()).is_err() {
            return false;
        }
        let mut response = [0u8; 32];
        let read = stream.read(&mut response).unwrap_or(0);
        response[..read].starts_with(b"HTTP/1.1 200")
            || response[..read].starts_with(b"HTTP/1.0 200")
    }

    fn terminate(&self, marker: &StaleServerMarker) -> std::io::Result<()> {
        terminate_process(marker.pid)
    }
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> std::io::Result<()> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "taskkill failed for PID {pid} with {status}"
        )))
    }
}

#[cfg(not(windows))]
fn terminate_process(pid: u32) -> std::io::Result<()> {
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "kill failed for PID {pid} with {status}"
        )))
    }
}

/// Production provisioner: provisions `<Course Workspace>/.venv`, then starts the
/// server from its interpreter. Blocking; call it from a background executor.
pub struct SystemJupyterLabProvisioner {
    runtime_dir: PathBuf,
    startup_timeout: Duration,
}

impl SystemJupyterLabProvisioner {
    pub fn new(runtime_dir: impl Into<PathBuf>) -> Self {
        Self {
            runtime_dir: runtime_dir.into(),
            startup_timeout: Duration::from_secs(60),
        }
    }
}

impl JupyterLabProvisioner for SystemJupyterLabProvisioner {
    fn start(&self, course: &crate::course::Course) -> Result<Box<dyn JupyterLabRuntime>, String> {
        let runner = crate::runtime::SystemCommandRunner;
        let environment =
            crate::runtime::RuntimeManager::for_course(&runner, course, extension_baseline())
                .provision()
                .map_err(|error| error.to_string())?;
        let config = JupyterLabServerConfig {
            interpreter: environment.interpreter,
            workspace: course.workspace_path(),
            server_port: 0,
            token: uuid::Uuid::new_v4().simple().to_string(),
            marker_path: StaleServerMarker::path(&self.runtime_dir),
            startup_timeout: self.startup_timeout,
        };
        let server = JupyterLabServer::start(
            config,
            &SystemJupyterLabSpawner,
            &TcpJupyterLabProbe,
            &SystemStaleServerCleanup,
        )
        .map_err(|error| error.to_string())?;
        Ok(Box::new(server))
    }

    fn cleanup_stale(&self) {
        cleanup_stale(
            &StaleServerMarker::path(&self.runtime_dir),
            &SystemStaleServerCleanup,
        );
    }
}

/// Resolution order: the explicit override, a debug build's local `dist/`
/// wheel, a wheel beside the packaged executable, then `wheels/` beside it.
fn extension_baseline() -> crate::runtime::CourseEnvironmentBaseline {
    let version = std::env::var("CYNAPSE_EXTENSION_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let wheel = std::env::var_os("CYNAPSE_EXTENSION_WHEEL")
        .map(PathBuf::from)
        .map(resolve_wheel_path)
        .unwrap_or_else(|| extension_wheel(&version));
    crate::runtime::CourseEnvironmentBaseline {
        jupyterlab_version: crate::runtime::BASELINE_JUPYTERLAB_VERSION.to_string(),
        extension_package: std::env::var("CYNAPSE_EXTENSION_PACKAGE")
            .unwrap_or_else(|_| crate::runtime::BASELINE_EXTENSION_PACKAGE.to_string()),
        extension_version: version,
        extension_wheel: wheel,
    }
}

/// Resolve a relative override from the process working directory before it is
/// passed to pip, whose cwd is the Course Workspace.
fn resolve_wheel_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    std::env::current_dir()
        .map(|directory| directory.join(&path))
        .unwrap_or(path)
}

fn extension_wheel_filename(version: &str) -> String {
    format!("cynapse_jupyterlab_extension-{version}-py3-none-any.whl")
}

#[cfg(any(not(debug_assertions), test))]
fn extension_wheel_from_executable_dir(directory: &Path, version: &str) -> PathBuf {
    let beside = directory.join(extension_wheel_filename(version));
    if beside.is_file() {
        return beside;
    }
    directory
        .join("wheels")
        .join(extension_wheel_filename(version))
}

#[cfg(debug_assertions)]
fn extension_wheel(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("adapters/jupyterlab/dist")
        .join(extension_wheel_filename(version))
}

#[cfg(not(debug_assertions))]
fn extension_wheel(version: &str) -> PathBuf {
    let directory = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    extension_wheel_from_executable_dir(&directory, version)
}

#[cfg(test)]
mod tests;
