//! JupyterLab supply state and lifecycle.
//!
//! The supply owns the running JupyterLab server and decides when it can be
//! reused. Protocol Server installation stays outside: callers pass a control
//! starter so this module does not depend on `SessionService`.

use super::protocol_server::ProtocolServerSlot;
use super::*;
use cynapse_client_core::JupyterLabProvisioner;
use std::future::Future;

pub(super) struct JupyterLabSupply {
    session: Mutex<Option<JupyterLabSession>>,
    provisioner: Arc<dyn JupyterLabProvisioner>,
    control_port: u16,
}

struct JupyterLabSession {
    workspace: PathBuf,
    server: Box<dyn cynapse_client_core::JupyterLabRuntime>,
}

pub(super) struct JupyterLabEndpoint {
    pub(super) url: String,
    pub(super) started: bool,
}

impl JupyterLabSupply {
    pub(super) fn new(provisioner: Arc<dyn JupyterLabProvisioner>, control_port: u16) -> Self {
        Self {
            session: Mutex::new(None),
            provisioner,
            control_port,
        }
    }

    pub(super) fn cleanup_stale(&self) {
        self.provisioner.cleanup_stale();
    }

    pub(super) fn control_port(&self) -> u16 {
        self.control_port
    }

    /// Starts or reuses the one JupyterLab server for a course. `start_control`
    /// installs the JupyterLab Protocol Server for a newly started or recovered
    /// control port.
    pub(super) async fn ensure<F, Fut>(
        &self,
        course: &Course,
        protocol: &ProtocolServerSlot,
        start_control: F,
    ) -> Result<JupyterLabEndpoint, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        let mut start_control = Some(start_control);
        let workspace = course.workspace_path();
        if let Some(url) = self.running_url(&workspace) {
            if !protocol.has_control_server(self.control_port()) {
                let origin = self
                    .origin()
                    .ok_or_else(|| t("error.jupyterlab_server_exited"))?;
                log::info!(
                    target: "cynapse.jupyterlab",
                    "server.control_restarted workspace={} url={}",
                    workspace.display(),
                    url
                );
                start_control.take().expect("control starter is available")(origin).await?;
            }
            log::info!(
                target: "cynapse.jupyterlab",
                "server.reused workspace={} url={}",
                workspace.display(),
                url
            );
            return Ok(JupyterLabEndpoint {
                url,
                started: false,
            });
        }
        self.stop();

        // shortcut: this preflight bind only fails fast; the real bind below
        // is authoritative.
        if self.control_port() != 0 {
            std::net::TcpListener::bind(("127.0.0.1", self.control_port())).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AddrInUse {
                    t_with(
                        "error.jupyterlab_control_port_in_use",
                        &[("port", &self.control_port().to_string())],
                    )
                } else {
                    t_with(
                        "error.jupyterlab_spawn_failed",
                        &[("error", &error.to_string())],
                    )
                }
            })?;
        }

        let provisioner = self.provisioner.clone();
        let course = course.clone();
        let started = tokio::task::spawn_blocking(move || provisioner.start(&course)).await;
        let mut runtime = match started {
            Ok(Ok(runtime)) => runtime,
            Ok(Err(error)) => return Err(error),
            Err(error) => {
                let detail = error.to_string();
                return Err(t_with(
                    "error.jupyterlab_task_failed",
                    &[("error", &detail)],
                ));
            }
        };
        let origin = format!("http://127.0.0.1:{}", runtime.server_port());
        if let Err(error) =
            start_control.take().expect("control starter is available")(origin).await
        {
            runtime.stop();
            return Err(error);
        }
        let url = runtime.browser_url();
        *self
            .session
            .lock()
            .map_err(|_| t("error.playback_server_unavailable"))? = Some(JupyterLabSession {
            workspace: runtime.workspace().to_path_buf(),
            server: runtime,
        });
        log::info!(
            target: "cynapse.jupyterlab",
            "server.started workspace={} url={}",
            workspace.display(),
            url
        );
        Ok(JupyterLabEndpoint { url, started: true })
    }

    fn running_url(&self, workspace: &Path) -> Option<String> {
        let mut guard = self.session.lock().ok()?;
        let session = guard.as_mut()?;
        (session.workspace == workspace && session.server.is_running())
            .then(|| session.server.browser_url())
    }

    pub(super) fn origin(&self) -> Option<String> {
        let mut guard = self.session.lock().ok()?;
        let session = guard.as_mut()?;
        session
            .server
            .is_running()
            .then(|| format!("http://127.0.0.1:{}", session.server.server_port()))
    }

    pub(super) fn stop(&self) {
        if let Ok(mut guard) = self.session.lock()
            && let Some(mut session) = guard.take()
        {
            session.server.stop();
        }
    }
}
