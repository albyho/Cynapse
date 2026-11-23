use super::store::{challenge_status_key, clear_resume_action, persist_resume_action};
use super::*;

/// Course progress recording, cursors, and lifecycle.
impl CourseSession {
    pub fn record_progress(
        &mut self,
        unit_path: &str,
        step_id: &str,
        step_completed: bool,
    ) -> Result<(), CourseSessionError> {
        self.mark_unit_changed(unit_path);
        let course_id = self.course_id()?;
        let (unit_complete, next_step) = {
            let course = self
                .active_course
                .as_ref()
                .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
            let progress = self
                .progress
                .as_ref()
                .ok_or(CourseSessionError::NoActiveProgress)?;
            let step_index = self
                .active_step_index()
                .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
            let unit_complete = step_index
                .step_indices(unit_path)
                .iter()
                .map(|index| &course.steps[*index])
                .all(|step| {
                    step.step.step_id == step_id && step_completed
                        || progress.progress.iter().any(|entry| {
                            entry.unit_path == unit_path
                                && entry.step_id == step.step.step_id
                                && entry.completed
                        })
                });
            let next_step = if !step_completed || unit_complete {
                None
            } else {
                step_index.following_playback_step(course, unit_path, step_id)
            };
            (unit_complete, next_step)
        };
        // Unit completion clears the resume cursor; every Unit is independent.
        let (current_unit_path, current_step_id) = match next_step {
            Some(step) => (Some(step.unit_path), Some(step.step_id)),
            None if unit_complete => (None, None),
            None => (Some(unit_path.to_string()), Some(step_id.to_string())),
        };
        let unit_status = if unit_complete {
            UnitStatus::Completed
        } else {
            UnitStatus::InProgress
        };
        let now = now_unix_millis() as i64;
        let connection = &self.connection;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "
            UPDATE course_progress
            SET current_unit_path = ?1,
                current_step_id = ?2, updated_at = ?3
            WHERE id = ?4
            ",
            rusqlite::params![current_unit_path, current_step_id, now, course_id],
        )?;
        transaction.execute(
            "
            INSERT INTO step_states(
                course_id, unit_path, step_id, started, completed, updated_at
            )
            VALUES (?3, ?4, ?5, 1, ?1, ?2)
            ON CONFLICT(course_id, unit_path, step_id) DO UPDATE SET
                started = 1,
                completed = excluded.completed,
                updated_at = excluded.updated_at
            ",
            rusqlite::params![
                i64::from(step_completed),
                now,
                course_id,
                unit_path,
                step_id,
            ],
        )?;
        transaction.execute(
            "
            INSERT INTO unit_states(
                course_id, unit_path, status,
                current_step_id, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(course_id, unit_path) DO UPDATE SET
                status = excluded.status,
                current_step_id = excluded.current_step_id,
                updated_at = excluded.updated_at
            ",
            rusqlite::params![
                course_id,
                unit_path,
                unit_status_key(unit_status),
                step_id,
                now,
            ],
        )?;
        transaction.commit()?;
        if step_completed {
            clear_resume_action(connection, &course_id)?;
        }

        if let Some(progress) = self.progress.as_mut() {
            progress.current_unit_path = current_unit_path;
            progress.current_step_id = current_step_id;
            progress.updated_at = now as u64;
            if let Some(step) = progress
                .progress
                .iter_mut()
                .find(|entry| entry.unit_path == unit_path && entry.step_id == step_id)
            {
                step.started = true;
                step.completed = step_completed;
            } else {
                progress.progress.push(StepProgressRecord {
                    unit_path: unit_path.to_string(),
                    step_id: step_id.to_string(),
                    started: true,
                    completed: step_completed,
                    challenge_status: ChallengeStatus::NotStarted,
                    challenge_attempts: 0,
                    challenge_checked_at: None,
                });
            }
            if let Some(state) = progress
                .unit_states
                .iter_mut()
                .find(|state| state.unit_path == unit_path)
            {
                state.status = unit_status;
                state.current_step_id = Some(step_id.to_string());
                state.updated_at = now as u64;
            } else {
                progress.unit_states.push(UnitStateRecord {
                    unit_path: unit_path.to_string(),
                    status: unit_status,
                    current_step_id: Some(step_id.to_string()),
                    updated_at: now as u64,
                });
            }
        }
        if unit_complete {
            // 完成的 Unit 不再保持选中；树里只保留“重置”，避免出现
            // “当前且已完成”这种既可取消选择又可重置的状态。
            self.selected_unit_path = None;
            persist_selection(connection, None)?;
        }
        Ok(())
    }

    pub fn record_resume_action(
        &mut self,
        unit_path: &str,
        step_id: &str,
        action_index: usize,
    ) -> Result<(), CourseSessionError> {
        let course_id = self.course_id()?;
        persist_resume_action(
            &self.connection,
            &course_id,
            unit_path,
            step_id,
            action_index,
        )
    }

    pub fn record_challenge_failure(
        &mut self,
        unit_path: &str,
        step_id: &str,
    ) -> Result<(), CourseSessionError> {
        self.update_challenge_status(unit_path, step_id, ChallengeStatus::Failed, 1, false)
    }

    pub fn record_challenge_pass(
        &mut self,
        unit_path: &str,
        step_id: &str,
    ) -> Result<(), CourseSessionError> {
        self.update_challenge_status(unit_path, step_id, ChallengeStatus::Passed, 1, true)?;
        self.record_progress(unit_path, step_id, true)
    }

    pub fn record_challenge_skip(
        &mut self,
        unit_path: &str,
        step_id: &str,
    ) -> Result<(), CourseSessionError> {
        self.update_challenge_status(unit_path, step_id, ChallengeStatus::Skipped, 0, true)?;
        self.record_progress(unit_path, step_id, true)
    }

    fn update_challenge_status(
        &mut self,
        unit_path: &str,
        step_id: &str,
        status: ChallengeStatus,
        attempts_delta: u32,
        completed: bool,
    ) -> Result<(), CourseSessionError> {
        self.mark_unit_changed(unit_path);
        let course_id = self.course_id()?;
        let now = now_unix_millis() as i64;
        let connection = &self.connection;
        connection.execute(
            "
            INSERT INTO step_states(
                course_id, unit_path, step_id, started, completed,
                challenge_status, challenge_attempts, challenge_checked_at, updated_at
            )
            VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?7)
            ON CONFLICT(course_id, unit_path, step_id) DO UPDATE SET
                started = 1,
                completed = excluded.completed,
                challenge_status = excluded.challenge_status,
                challenge_attempts = step_states.challenge_attempts + excluded.challenge_attempts,
                challenge_checked_at = excluded.challenge_checked_at,
                updated_at = excluded.updated_at
            ",
            rusqlite::params![
                course_id,
                unit_path,
                step_id,
                i64::from(completed),
                challenge_status_key(status),
                i64::from(attempts_delta),
                now,
            ],
        )?;
        if let Some(progress) = self.progress.as_mut() {
            if let Some(entry) = progress
                .progress
                .iter_mut()
                .find(|entry| entry.unit_path == unit_path && entry.step_id == step_id)
            {
                entry.started = true;
                entry.completed = completed;
                entry.challenge_status = status;
                entry.challenge_attempts = entry.challenge_attempts.saturating_add(attempts_delta);
                entry.challenge_checked_at = Some(now as u64);
            } else {
                progress.progress.push(StepProgressRecord {
                    unit_path: unit_path.to_string(),
                    step_id: step_id.to_string(),
                    started: true,
                    completed,
                    challenge_status: status,
                    challenge_attempts: attempts_delta,
                    challenge_checked_at: Some(now as u64),
                });
            }
        }
        Ok(())
    }

    pub(super) fn set_unit_position(&mut self, unit_path: &str) -> Result<(), CourseSessionError> {
        self.mark_unit_changed(unit_path);
        let course_id = self.course_id()?;
        let (step_id, keep_recovery) = {
            let progress = self
                .progress
                .as_ref()
                .ok_or(CourseSessionError::NoActiveProgress)?;
            let keep_recovery = progress.current_unit_path.as_deref() == Some(unit_path);
            let course = self
                .active_course
                .as_ref()
                .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
            let steps = course
                .steps
                .iter()
                .filter(|step| step.unit_path == unit_path)
                .collect::<Vec<_>>();
            let step = steps
                .iter()
                .find(|step| {
                    !progress.progress.iter().any(|entry| {
                        entry.unit_path == unit_path
                            && entry.step_id == step.step.step_id
                            && entry.completed
                    })
                })
                .or_else(|| steps.first())
                .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;
            (step.step.step_id.clone(), keep_recovery)
        };
        let now = now_unix_millis() as i64;
        let connection = &self.connection;
        connection.execute(
            "
            UPDATE course_progress
            SET current_unit_path = ?1, current_step_id = ?2, updated_at = ?3
            WHERE id = ?4
            ",
            rusqlite::params![unit_path, step_id, now, course_id],
        )?;
        if let Some(progress) = self.progress.as_mut() {
            progress.current_unit_path = Some(unit_path.to_string());
            progress.current_step_id = Some(step_id);
            progress.updated_at = now as u64;
            if !keep_recovery {
                progress.recovery = None;
            }
        }
        Ok(())
    }

    pub(super) fn ensure_progress(&mut self) -> Result<(), CourseSessionError> {
        if self.progress.is_some() {
            return Ok(());
        }
        let course = self
            .active_course
            .as_ref()
            .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
        let course_path = self
            .active_path
            .clone()
            .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
        let course_id = course.manifest.id.clone();
        let version = course.manifest.version.clone();
        let workspace_path = course.workspace_path().to_string_lossy().to_string();
        super::prepare_course_resources(Path::new(&course_path), Path::new(&workspace_path))?;
        let now = now_unix_millis();
        let connection = &self.connection;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM course_progress WHERE id = ?1", [&course_id])?;
        transaction.execute(
            "
            INSERT INTO course_progress(
                id, course_path, version,
                current_unit_path, current_step_id, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?4)
            ",
            rusqlite::params![course_id, course_path, version, now as i64],
        )?;
        let mut insert_step = transaction.prepare(
            "
            INSERT INTO step_states(
                course_id, unit_path, step_id, started, completed, updated_at
            )
            VALUES (?1, ?2, ?3, 0, 0, ?4)
            ",
        )?;
        for step in &course.steps {
            insert_step.execute(rusqlite::params![
                course_id,
                step.unit_path,
                step.step.step_id,
                now as i64,
            ])?;
        }
        drop(insert_step);
        let mut insert_state = transaction.prepare(
            "
            INSERT INTO unit_states(
                course_id, unit_path, status,
                current_step_id, updated_at
            )
            VALUES (?1, ?2, 'not_started', NULL, ?3)
            ",
        )?;
        for entry in &course.units {
            insert_state.execute(rusqlite::params![course_id, entry.path, now as i64])?;
        }
        drop(insert_state);
        transaction.commit()?;
        self.running_command = false;
        self.reload_progress()?;
        Ok(())
    }

    pub(super) fn clear_unit_step_rows(
        &mut self,
        unit_path: &str,
    ) -> Result<(), CourseSessionError> {
        let course_id = self.course_id()?;
        let connection = &self.connection;
        connection.execute(
            "DELETE FROM step_states WHERE course_id = ?1 AND unit_path = ?2",
            rusqlite::params![course_id, unit_path],
        )?;
        self.reload_progress()?;
        Ok(())
    }

    pub(super) fn reload_progress(&mut self) -> Result<(), CourseSessionError> {
        let connection = &self.connection;
        let Some(id) = self
            .active_course
            .as_ref()
            .map(|course| course.manifest.id.clone())
        else {
            self.progress = None;
            return Ok(());
        };
        self.progress = load_progress(connection, &id)?;
        Ok(())
    }

    pub(super) fn reload_active_progress(
        &mut self,
    ) -> Result<Option<CourseProgress>, CourseSessionError> {
        self.reload_progress()?;
        Ok(self.progress.clone())
    }

    pub(super) fn course_id(&self) -> Result<String, CourseSessionError> {
        self.active_course
            .as_ref()
            .map(|course| course.manifest.id.clone())
            .ok_or(CourseSessionError::NoActiveProgress)
    }
}
