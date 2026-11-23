//! Course open, selection, and close use cases.

use super::*;

impl SessionService {
    /// Opens an explicit startup course before the protocol server starts.
    ///
    /// A failure leaves the restored Active Course in place; the caller
    /// surfaces it as a startup warning.
    pub fn open_course_at_startup(&self, path: PathBuf) -> Result<CourseSessionState, String> {
        let mut session = self
            .inner
            .session
            .lock()
            .map_err(|_| t("error.course_session_unavailable"))?;
        session.open_course(path).map_err(|error| error.to_string())
    }

    pub async fn open_course(&self, path: PathBuf) -> Result<CourseSessionState, String> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Opening,
            ClientTransitionRequirement::AdapterIndependent,
            t("transition.opening"),
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            let warnings = self.detach_for_course_switch().await;
            if !warnings.is_empty() {
                let _ = self.inner.events.send(ClientEvent::Warnings(warnings));
            }
            let state = {
                let mut session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                session
                    .open_course(&path)
                    .map_err(|error| error.to_string())
            };
            let state = match state {
                Ok(state) => state,
                Err(error) => {
                    if let Err(restart_error) = self.restart_server_on_runtime().await {
                        let _ = self
                            .inner
                            .events
                            .send(ClientEvent::Warnings(vec![restart_error.into()]));
                    }
                    return Err(error);
                }
            };
            let _ = self
                .inner
                .events
                .send(ClientEvent::Course(Arc::new(state.clone())));
            self.restart_server_on_runtime().await?;
            Ok(state)
        }
        .await;
        self.finish_transition(ticket);
        result
    }

    pub async fn select_unit(&self, unit_path: &str) -> Result<CourseSessionState, String> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Selecting,
            ClientTransitionRequirement::AdapterIndependent,
            t("transition.selecting"),
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            let state = {
                let mut session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                session
                    .select_unit(unit_path)
                    .map_err(|error| error.to_string())?
            };
            let _ = self
                .inner
                .events
                .send(ClientEvent::Course(Arc::new(state.clone())));
            Ok(state)
        }
        .await;
        self.finish_transition(ticket);
        result
    }

    pub async fn clear_selection(&self) -> Result<CourseSessionState, String> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Selecting,
            ClientTransitionRequirement::AdapterIndependent,
            t("transition.cancelling_selection"),
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            let state = {
                let mut session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                session
                    .clear_selection()
                    .map_err(|error| error.to_string())?
            };
            let _ = self
                .inner
                .events
                .send(ClientEvent::Course(Arc::new(state.clone())));
            Ok(state)
        }
        .await;
        self.finish_transition(ticket);
        result
    }

    /// Removes recent-course history without changing the active Course.
    pub fn clear_recent_courses(&self) -> Result<CourseSessionState, String> {
        let state = {
            let mut session = self
                .inner
                .session
                .lock()
                .map_err(|_| t("error.course_session_unavailable"))?;
            session
                .clear_recent_courses()
                .map_err(|error| error.to_string())?
        };
        let _ = self
            .inner
            .events
            .send(ClientEvent::Course(Arc::new(state.clone())));
        Ok(state)
    }

    /// Returns the Client to the no-course state. Progress, recent courses, and
    /// Unit workspaces stay on disk so reopening the package can resume.
    pub async fn close_course(&self) -> Result<CourseSessionState, String> {
        let ticket = self.start_latest_transition(
            ClientTransitionKind::Preparing,
            ClientTransitionRequirement::AdapterIndependent,
            t("transition.closing"),
        )?;
        let result = async {
            self.wait_for_transition(&ticket).await?;
            let warnings = self.detach_for_course_switch().await;
            if !warnings.is_empty() {
                let _ = self.inner.events.send(ClientEvent::Warnings(warnings));
            }
            let state = {
                let mut session = self
                    .inner
                    .session
                    .lock()
                    .map_err(|_| t("error.course_session_unavailable"))?;
                session.close_course().map_err(|error| error.to_string())
            };
            let state = match state {
                Ok(state) => state,
                Err(error) => {
                    if let Err(restart_error) = self.restart_server_on_runtime().await {
                        let _ = self
                            .inner
                            .events
                            .send(ClientEvent::Warnings(vec![restart_error.into()]));
                    }
                    return Err(error);
                }
            };
            let _ = self
                .inner
                .events
                .send(ClientEvent::Course(Arc::new(state.clone())));
            self.restart_server_on_runtime().await?;
            Ok(state)
        }
        .await;
        self.finish_transition(ticket);
        result
    }
}
