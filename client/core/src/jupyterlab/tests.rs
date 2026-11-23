use super::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

struct FakeProcess {
    pid: u32,
    alive: Arc<AtomicBool>,
}

impl JupyterLabProcess for FakeProcess {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn has_exited(&mut self) -> bool {
        !self.alive.load(Ordering::SeqCst)
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.alive.store(false, Ordering::SeqCst);
        Ok(())
    }
}

struct FakeSpawner {
    launches: Mutex<Vec<JupyterLabLaunch>>,
    alive: Arc<AtomicBool>,
    next_pid: AtomicU32,
}

impl FakeSpawner {
    fn new() -> Self {
        Self {
            launches: Mutex::new(Vec::new()),
            alive: Arc::new(AtomicBool::new(true)),
            next_pid: AtomicU32::new(100),
        }
    }

    fn launches(&self) -> Vec<JupyterLabLaunch> {
        self.launches.lock().unwrap().clone()
    }
}

impl JupyterLabSpawner for FakeSpawner {
    fn spawn(&self, launch: &JupyterLabLaunch) -> std::io::Result<Box<dyn JupyterLabProcess>> {
        self.launches.lock().unwrap().push(launch.clone());
        Ok(Box::new(FakeProcess {
            pid: self.next_pid.fetch_add(1, Ordering::SeqCst),
            alive: self.alive.clone(),
        }))
    }
}

struct FakeProbe {
    ready: Arc<AtomicBool>,
}

impl JupyterLabReadinessProbe for FakeProbe {
    fn is_ready(&self, _port: u16) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

struct FakeStale {
    live: bool,
    terminated: Arc<AtomicBool>,
}

impl StaleServerCleanup for FakeStale {
    fn is_live(&self, _marker: &StaleServerMarker) -> bool {
        self.live
    }

    fn terminate(&self, _marker: &StaleServerMarker) -> std::io::Result<()> {
        self.terminated.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn config(directory: &Path) -> JupyterLabServerConfig {
    JupyterLabServerConfig {
        interpreter: directory.join(".venv/Scripts/python.exe"),
        workspace: directory.join("workspace"),
        server_port: 0,
        token: "launch-token".to_string(),
        marker_path: directory.join(STALE_SERVER_MARKER_FILE),
        startup_timeout: Duration::from_millis(200),
    }
}

fn start(
    config: JupyterLabServerConfig,
    spawner: &FakeSpawner,
    ready: &Arc<AtomicBool>,
    stale: &FakeStale,
) -> Result<JupyterLabServer, JupyterLabStartError> {
    JupyterLabServer::start(
        config,
        spawner,
        &FakeProbe {
            ready: ready.clone(),
        },
        stale,
    )
}

#[cfg(debug_assertions)]
#[test]
fn a_debug_build_uses_the_local_dist_wheel() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    assert_eq!(
        extension_wheel("0.2.4"),
        root.join("adapters/jupyterlab/dist/cynapse_jupyterlab_extension-0.2.4-py3-none-any.whl")
    );
}

#[test]
fn a_relative_wheel_override_is_resolved_from_the_process_directory() {
    let relative = std::path::PathBuf::from("wheels/extension.whl");
    let resolved = resolve_wheel_path(relative);
    assert!(resolved.is_absolute());
    assert!(resolved.ends_with("wheels/extension.whl"));
}

#[test]
fn a_packaged_build_prefers_a_wheel_beside_the_executable() {
    let directory = tempfile::tempdir().unwrap();
    let wheel = directory
        .path()
        .join("cynapse_jupyterlab_extension-0.2.4-py3-none-any.whl");
    std::fs::write(&wheel, b"wheel").unwrap();

    assert_eq!(
        extension_wheel_from_executable_dir(directory.path(), "0.2.4"),
        wheel
    );
}

#[test]
fn a_packaged_build_falls_back_to_wheels_beside_the_executable() {
    let directory = tempfile::tempdir().unwrap();

    assert_eq!(
        extension_wheel_from_executable_dir(directory.path(), "0.2.4"),
        directory
            .path()
            .join("wheels/cynapse_jupyterlab_extension-0.2.4-py3-none-any.whl")
    );
}

#[test]
fn browser_url_carries_only_the_jupyter_token() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = config(directory.path());
    config.server_port = 18890;

    assert_eq!(
        config.browser_url(18890),
        "http://127.0.0.1:18890/lab?token=launch-token"
    );
}

#[test]
fn launch_uses_the_course_interpreter_workspace_and_token() {
    let directory = tempfile::tempdir().unwrap();
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(true));
    let stale = FakeStale {
        live: false,
        terminated: Arc::new(AtomicBool::new(false)),
    };
    let mut config = config(directory.path());
    config.server_port = 18890;
    let server = start(config, &spawner, &ready, &stale).unwrap();

    let launches = spawner.launches();
    assert_eq!(launches.len(), 1);
    let launch = &launches[0];
    assert_eq!(
        launch.program,
        directory.path().join(".venv/Scripts/python.exe")
    );
    assert_eq!(launch.cwd, directory.path().join("workspace"));
    assert!(launch.args.iter().any(|arg| arg == "--no-browser"));
    assert!(
        launch
            .args
            .iter()
            .any(|arg| arg == "--ServerApp.port=18890")
    );
    assert!(
        launch
            .args
            .iter()
            .any(|arg| arg == "--ServerApp.ip=127.0.0.1")
    );
    assert!(
        launch
            .args
            .iter()
            .any(|arg| { arg.starts_with("--ServerApp.root_dir=") && arg.ends_with("workspace") })
    );
    assert!(
        launch
            .args
            .iter()
            .any(|arg| arg == "--ServerApp.token=launch-token")
    );
    assert!(
        launch
            .args
            .iter()
            .any(|arg| arg == "--MappingKernelManager.default_kernel_name=python3")
    );
    assert_eq!(server.server_port(), 18890);
    assert_eq!(server.workspace(), directory.path().join("workspace"));
}

#[test]
fn launch_puts_the_course_scripts_first_on_path_and_sets_virtual_env() {
    let directory = tempfile::tempdir().unwrap();
    let scripts = directory.path().join(".venv").join("Scripts");
    let interpreter = scripts.join("python.exe");
    let env = course_environment_launch_env(&interpreter);

    let virtual_env = env
        .iter()
        .find(|(key, _)| key == "VIRTUAL_ENV")
        .map(|(_, value)| PathBuf::from(value))
        .expect("VIRTUAL_ENV is part of the launch environment");
    assert_eq!(virtual_env, directory.path().join(".venv"));

    let path = env
        .iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| std::env::split_paths(value).collect::<Vec<_>>())
        .expect("PATH is part of the launch environment");
    assert_eq!(path.first(), Some(&scripts));
    if let Some(existing) = std::env::var_os("PATH") {
        let existing: Vec<_> = std::env::split_paths(&existing).collect();
        assert_eq!(&path[1..], &existing[..]);
    }
}

#[test]
fn startup_writes_auto_start_default_kernel_to_the_course_venv() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path());
    let tracker = directory.path().join(
        ".venv/share/jupyter/lab/user-settings/@jupyterlab/notebook-extension/tracker.jupyterlab-settings",
    );
    std::fs::create_dir_all(tracker.parent().unwrap()).unwrap();
    std::fs::write(
        &tracker,
        "{\n  \"autoStartDefaultKernel\": false,\n  \"other\": true\n}\n",
    )
    .unwrap();

    ensure_jupyterlab_tracker_settings(&config).unwrap();

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&tracker).unwrap()).unwrap();
    assert_eq!(value["autoStartDefaultKernel"], true);
    assert_eq!(value["other"], true);

    let launch = config.launch(4_000);
    let settings = launch
        .env
        .iter()
        .find(|(key, _)| key == "JUPYTERLAB_SETTINGS_DIR")
        .map(|(_, value)| PathBuf::from(value))
        .expect("settings directory is part of the launch environment");
    assert_eq!(
        settings,
        directory
            .path()
            .join(".venv/share/jupyter/lab/user-settings")
    );
}

#[test]
fn a_free_port_is_chosen_when_the_config_asks_for_one() {
    let directory = tempfile::tempdir().unwrap();
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(true));
    let stale = FakeStale {
        live: false,
        terminated: Arc::new(AtomicBool::new(false)),
    };
    let server = start(config(directory.path()), &spawner, &ready, &stale).unwrap();

    assert!(server.server_port() > 0);
    let launch = &spawner.launches()[0];
    assert!(
        launch
            .args
            .iter()
            .any(|arg| arg == &format!("--ServerApp.port={}", server.server_port()))
    );
}

#[test]
fn a_live_stale_server_is_terminated_before_the_new_launch() {
    let directory = tempfile::tempdir().unwrap();
    let marker = StaleServerMarker {
        pid: 42,
        port: 18891,
        token: "old-token".to_string(),
        workspace: directory.path().join("old-workspace"),
    };
    std::fs::write(
        config(directory.path()).marker_path,
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(true));
    let terminated = Arc::new(AtomicBool::new(false));
    let stale = FakeStale {
        live: true,
        terminated: terminated.clone(),
    };

    let _server = start(config(directory.path()), &spawner, &ready, &stale).unwrap();

    assert!(
        terminated.load(Ordering::SeqCst),
        "a confirmed live stale server must be terminated"
    );
    let current = StaleServerMarker::read(&config(directory.path()).marker_path).unwrap();
    assert_ne!(current.pid, 42);
}

#[test]
fn a_dead_stale_server_marker_is_only_removed() {
    let directory = tempfile::tempdir().unwrap();
    let marker = StaleServerMarker {
        pid: 43,
        port: 18892,
        token: "old-token".to_string(),
        workspace: directory.path().join("old-workspace"),
    };
    std::fs::write(
        config(directory.path()).marker_path,
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(true));
    let terminated = Arc::new(AtomicBool::new(false));
    let stale = FakeStale {
        live: false,
        terminated: terminated.clone(),
    };

    let _server = start(config(directory.path()), &spawner, &ready, &stale).unwrap();

    assert!(!terminated.load(Ordering::SeqCst));
    assert_eq!(spawner.launches().len(), 1);
}

#[test]
fn a_busy_jupyter_port_is_an_actionable_failure_without_spawning() {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut config = config(directory.path());
    config.server_port = port;
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(true));
    let stale = FakeStale {
        live: false,
        terminated: Arc::new(AtomicBool::new(false)),
    };

    let error = match start(config, &spawner, &ready, &stale) {
        Ok(_) => panic!("a busy port must fail"),
        Err(error) => error,
    };

    assert!(matches!(error, JupyterLabStartError::PortInUse { .. }));
    assert!(error.to_string().contains(&port.to_string()));
    assert!(spawner.launches().is_empty());
    drop(listener);
}

#[test]
fn a_server_that_never_becomes_ready_times_out_and_leaves_no_marker() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = config(directory.path());
    config.startup_timeout = Duration::from_millis(30);
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(false));
    let stale = FakeStale {
        live: false,
        terminated: Arc::new(AtomicBool::new(false)),
    };

    let error = match start(config.clone(), &spawner, &ready, &stale) {
        Ok(_) => panic!("a server that never binds must fail"),
        Err(error) => error,
    };

    assert!(matches!(error, JupyterLabStartError::NotReady { .. }));
    assert!(error.to_string().contains("30") || error.to_string().contains("0"));
    assert!(
        !spawner.alive.load(Ordering::SeqCst),
        "failed start must kill the child"
    );
    assert!(!config.marker_path.exists());
}

#[test]
fn a_server_that_exits_during_startup_is_reported_as_exited() {
    let directory = tempfile::tempdir().unwrap();
    let spawner = FakeSpawner::new();
    spawner.alive.store(false, Ordering::SeqCst);
    let ready = Arc::new(AtomicBool::new(false));
    let stale = FakeStale {
        live: false,
        terminated: Arc::new(AtomicBool::new(false)),
    };
    let config = config(directory.path());

    let error = match start(config.clone(), &spawner, &ready, &stale) {
        Ok(_) => panic!("an exited server must not report ready"),
        Err(error) => error,
    };

    assert!(matches!(error, JupyterLabStartError::Exited));
    assert!(!config.marker_path.exists());
}

#[test]
fn stop_kills_the_process_and_removes_the_stale_marker() {
    let directory = tempfile::tempdir().unwrap();
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(true));
    let stale = FakeStale {
        live: false,
        terminated: Arc::new(AtomicBool::new(false)),
    };
    let mut server = start(config(directory.path()), &spawner, &ready, &stale).unwrap();
    assert!(config(directory.path()).marker_path.exists());
    assert!(server.is_running());

    server.stop();

    assert!(!server.is_running());
    assert!(!spawner.alive.load(Ordering::SeqCst));
    assert!(!config(directory.path()).marker_path.exists());
}

#[test]
fn a_server_process_that_exits_is_not_running() {
    let directory = tempfile::tempdir().unwrap();
    let spawner = FakeSpawner::new();
    let ready = Arc::new(AtomicBool::new(true));
    let stale = FakeStale {
        live: false,
        terminated: Arc::new(AtomicBool::new(false)),
    };
    let mut server = start(config(directory.path()), &spawner, &ready, &stale).unwrap();

    spawner.alive.store(false, Ordering::SeqCst);

    assert!(!server.is_running());
}

#[test]
fn the_stale_probe_requires_the_recorded_jupyter_token() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let marker = StaleServerMarker {
        pid: 44,
        port,
        token: "recorded-token".to_string(),
        workspace: PathBuf::from("workspace"),
    };
    let probe = SystemStaleServerCleanup;
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 512];
        let read = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        request
    });

    assert!(probe.is_live(&marker));
    let request = handle.join().unwrap();
    assert!(request.contains("GET /lab?token=recorded-token"));
}
