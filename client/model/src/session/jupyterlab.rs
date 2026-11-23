//! JupyterLab startup preparation at the SessionService boundary.

use super::*;

impl SessionService {
    /// Starts or reuses the one JupyterLab server for the Active Course, opens
    /// the browser with its per-launch token, and waits for the extension.
    pub(super) async fn prepare_jupyterlab(&self, unit_path: &str) -> Result<(), String> {
        let required = {
            let session = self
                .inner
                .session
                .lock()
                .map_err(|_| t("error.course_session_unavailable"))?;
            session
                .required_adapters_for_unit(unit_path)
                .contains(&AdapterId::Jupyterlab)
        };
        if !required {
            return Ok(());
        }

        let course = {
            let session = self
                .inner
                .session
                .lock()
                .map_err(|_| t("error.course_session_unavailable"))?;
            session
                .active_course()
                .cloned()
                .ok_or_else(|| t("error.course_workspace_unavailable"))?
        };
        let service = self.clone();
        let endpoint =
            self.inner
                .jupyterlab
                .ensure(
                    &course,
                    &self.inner.protocol_server,
                    move |origin| async move {
                        service.ensure_jupyterlab_protocol_server(&origin).await
                    },
                )
                .await?;
        let (url, started) = (endpoint.url, endpoint.started);
        let connection = self.connection_state();
        let connected = matches!(connection, ConnectionState::Connected { .. });
        let open_browser = started || !connected;
        log::info!(
            target: "cynapse.jupyterlab",
            "prepare unit={} server_started={} connection={:?} open_browser={} url={}",
            unit_path,
            started,
            connection,
            open_browser,
            url
        );
        // Reuse the open browser tab for the next Unit. Opening the same
        // JupyterLab URL on every Start creates a new tab and makes the
        // previous tab inactive.
        if open_browser {
            let _ = self.inner.events.send(ClientEvent::OpenUrl(url));
        }
        self.update_starting_stage(
            ClientTransitionStage::ConnectAdapter,
            t("startup.step.connection"),
        );
        if !self
            .wait_for_server_connection(self.inner.jupyterlab_handshake_timeout)
            .await
        {
            return Err(t_with(
                "error.jupyterlab_extension_timeout",
                &[(
                    "seconds",
                    &self
                        .inner
                        .jupyterlab_handshake_timeout
                        .as_secs()
                        .to_string(),
                )],
            ));
        }
        Ok(())
    }

    pub(super) async fn ensure_jupyterlab_protocol_server(
        &self,
        origin: &str,
    ) -> Result<(), String> {
        self.replace_server_for_adapter(
            AdapterId::Jupyterlab,
            ReplacementKind::Controlled,
            Some(origin),
        )
        .await
    }
}
