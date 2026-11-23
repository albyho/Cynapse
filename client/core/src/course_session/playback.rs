use super::*;

/// Playback planning: steps, narration assets, resume, unit transitions.
use super::store::load_resume_action;
impl CourseSession {
    pub fn action_narration_audio(
        &self,
        unit_path: &str,
        step_id: &str,
        action_index: usize,
        phase: ActionNarrationPhase,
        clip_index: usize,
    ) -> Result<Option<ActionNarrationAudioView>, CourseSessionError> {
        let Some(course) = self.active_course.as_ref() else {
            return Ok(None);
        };
        Ok(course
            .action_narration(unit_path, step_id, action_index, phase, clip_index)
            .map(|narration| ActionNarrationAudioView {
                audio_path: narration
                    .audio_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                offset_ms: narration.offset_ms,
                duration_ms: narration.duration_ms,
                caption: narration.caption.clone(),
            }))
    }

    pub fn playback_steps(&self) -> Vec<PlaybackStep> {
        self.prepared_playback_steps().unwrap_or_else(|error| {
            log::error!(
                target: "cynapse.playback",
                "could not prepare playback steps: {error}"
            );
            Vec::new()
        })
    }

    /// Fallible playback planning for production callers. `playback_steps` is
    /// kept for tests, but must not hide preparation failures from the UI.
    pub fn try_playback_steps(&self) -> Result<Vec<PlaybackStep>, CourseSessionError> {
        self.prepared_playback_steps()
    }

    /// The Challenge declaration for one Step in the active course.
    pub fn challenge(&self, unit_path: &str, step_id: &str) -> Option<crate::course::Challenge> {
        self.active_course
            .as_ref()?
            .challenge(unit_path, step_id)
            .cloned()
    }

    /// The persisted Challenge Status for one Step, if progress exists.
    pub fn challenge_status(&self, unit_path: &str, step_id: &str) -> Option<ChallengeStatus> {
        self.progress
            .as_ref()?
            .progress
            .iter()
            .find(|entry| entry.unit_path == unit_path && entry.step_id == step_id)
            .map(|entry| entry.challenge_status)
    }

    fn next_unit_after(&self, unit_path: &str) -> Result<Option<UnitLocation>, CourseSessionError> {
        let course = self
            .active_course
            .as_ref()
            .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;
        let index = course
            .units
            .iter()
            .position(|entry| entry.path == unit_path)
            .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;
        Ok(course
            .units
            .get(index + 1)
            .map(|entry| UnitLocation::new(entry.path.clone())))
    }

    pub fn continuous_transition_after(
        &self,
        unit_path: &str,
    ) -> Result<Option<UnitTransitionPlan>, CourseSessionError> {
        let Some(location) = self.next_unit_after(unit_path)? else {
            return Ok(None);
        };
        let environment_requirements = self.environment_requirements_for_unit(&location.path);
        let adapters = self.required_adapters_for_unit(&location.path);
        let reset_required = self
            .progress
            .as_ref()
            .and_then(|progress| {
                progress
                    .unit_states
                    .iter()
                    .find(|state| state.unit_path == location.path)
            })
            .is_some_and(|state| state.status != UnitStatus::NotStarted);
        let (reset_workspace_paths, reset_exclude_paths) = if reset_required {
            let scope = self.reset_scope_for_unit(&location.path)?;
            (scope.workspace_paths, scope.exclude_paths)
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(Some(UnitTransitionPlan {
            location,
            environment_requirements,
            adapters,
            reset_required,
            reset_workspace_paths,
            reset_exclude_paths,
        }))
    }

    pub fn start_continuous_transition(
        &mut self,
        plan: &UnitTransitionPlan,
    ) -> Result<UnitPlaybackPlan, CourseSessionError> {
        if plan.reset_required {
            self.reset_unit(&plan.location.path, true)?;
        }
        self.start_unit(&plan.location.path, false)?;
        Ok(UnitPlaybackPlan {
            steps: self.prepared_playback_steps()?,
            resume: self.playback_resume(),
        })
    }

    pub(super) fn prepared_playback_steps(&self) -> Result<Vec<PlaybackStep>, CourseSessionError> {
        let Some(selected_unit_path) = self.selected_unit_path.as_deref() else {
            return Ok(Vec::new());
        };
        let Some(progress) = self.progress.as_ref() else {
            return Ok(Vec::new());
        };
        if progress.current_unit_path.as_deref() != Some(selected_unit_path) {
            return Ok(Vec::new());
        }
        if self.unit_completed(selected_unit_path) {
            return Ok(Vec::new());
        }
        let course = self
            .active_course
            .as_ref()
            .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
        let step_index = self
            .active_step_index()
            .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
        let resource_root = self
            .active_workspace_path
            .as_ref()
            .map(|workspace| workspace.join("resources"))
            .unwrap_or_default();
        step_index
            .step_indices(selected_unit_path)
            .iter()
            .filter_map(|index| course.playback_step(*index))
            .map(|mut step| {
                for action in &mut step.actions {
                    action.action = prepare_unit_action(
                        course,
                        &action.action,
                        selected_unit_path,
                        &resource_root,
                    )?;
                }
                Ok(step)
            })
            .collect()
    }

    pub fn first_unit_path(&self) -> Option<String> {
        self.active_course
            .as_ref()?
            .units
            .first()
            .map(|entry| entry.path.clone())
    }

    pub fn playback_resume(&self) -> Option<PlaybackResume> {
        let progress = self.progress.as_ref()?;
        let unit_path = progress.current_unit_path.clone()?;
        let step_id = progress.current_step_id.clone()?;
        if self.unit_completed(&unit_path) {
            return None;
        }
        let course = self.active_course.as_ref()?;
        self.active_step_index()?
            .step(course, &unit_path, &step_id)?;
        let recovery = progress
            .recovery
            .as_ref()
            .filter(|recovery| recovery.unit_path == unit_path && recovery.step_id == step_id)
            .map(|recovery| crate::server::PlaybackRecovery {
                reason: recovery.reason,
                command: recovery.command.clone(),
                output: recovery.output.clone(),
                output_truncated: recovery.output_truncated,
                exit_code: recovery.exit_code,
            });
        let action_index = self.course_id().ok().and_then(|course_id| {
            load_resume_action(&self.connection, &course_id, &unit_path, &step_id)
                .ok()
                .flatten()
        });
        Some(PlaybackResume {
            unit_path,
            step_id,
            step_started: progress.progress.iter().any(|entry| {
                entry.unit_path == progress.current_unit_path.as_deref().unwrap_or_default()
                    && entry.step_id == progress.current_step_id.as_deref().unwrap_or_default()
                    && entry.started
            }),
            action_index,
            recovery,
        })
    }
}
