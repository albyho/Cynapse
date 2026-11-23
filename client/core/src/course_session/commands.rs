use super::*;
use cynapse_client_i18n::t;

/// Terminal command recording, interruption, and completion.
impl CourseSession {
    pub fn record_command_started(
        &mut self,
        started: CommandExecutionStart<'_>,
    ) -> Result<(), CourseSessionError> {
        let CommandExecutionStart {
            unit_path,
            step_id,
            action_index,
            command,
            cwd,
            timeout_ms,
        } = started;
        self.mark_unit_changed(unit_path);
        let course_id = self.course_id()?;
        let now = now_unix_millis() as i64;
        let connection = &self.connection;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "
            INSERT INTO command_executions(
                course_id, unit_path, step_id, action_index,
                command, cwd, timeout_ms, status, output, output_truncated, exit_code, interruption,
                started_at, updated_at
            )
            VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running',
                NULL, 0, NULL, NULL, ?8, ?8
            )
            ON CONFLICT(course_id, unit_path, step_id, action_index) DO UPDATE SET
                command = excluded.command,
                cwd = excluded.cwd,
                timeout_ms = excluded.timeout_ms,
                status = 'running',
                output = NULL,
                output_truncated = 0,
                exit_code = NULL,
                interruption = NULL,
                started_at = excluded.started_at,
                updated_at = excluded.updated_at
            ",
            rusqlite::params![
                course_id,
                unit_path,
                step_id,
                action_index as i64,
                command,
                cwd,
                timeout_ms as i64,
                now,
            ],
        )?;
        transaction.execute(
            "
            UPDATE course_progress
            SET current_unit_path = ?1,
                current_step_id = ?2, updated_at = ?3
            WHERE id = ?4
            ",
            rusqlite::params![unit_path, step_id, now, course_id],
        )?;
        transaction.execute(
            "
            INSERT INTO unit_states(
                course_id, unit_path, status,
                current_step_id, updated_at
            )
            VALUES (?1, ?2, 'in_progress', ?3, ?4)
            ON CONFLICT(course_id, unit_path) DO UPDATE SET
                status = 'in_progress',
                current_step_id = excluded.current_step_id,
                updated_at = excluded.updated_at
            ",
            rusqlite::params![course_id, unit_path, step_id, now],
        )?;
        transaction.commit()?;
        if let Some(progress) = self.progress.as_mut() {
            progress.current_unit_path = Some(unit_path.to_string());
            progress.current_step_id = Some(step_id.to_string());
            progress.updated_at = now as u64;
            progress.recovery = None;
            if let Some(state) = progress
                .unit_states
                .iter_mut()
                .find(|state| state.unit_path == unit_path)
            {
                state.status = UnitStatus::InProgress;
                state.current_step_id = Some(step_id.to_string());
                state.updated_at = now as u64;
            } else {
                progress.unit_states.push(UnitStateRecord {
                    unit_path: unit_path.to_string(),
                    status: UnitStatus::InProgress,
                    current_step_id: Some(step_id.to_string()),
                    updated_at: now as u64,
                });
            }
        }
        self.running_command = true;
        Ok(())
    }

    pub fn record_command_interrupted(
        &mut self,
        interruption: CommandInterruption,
    ) -> Result<(), CourseSessionError> {
        let CommandInterruption {
            unit_path,
            step_id,
            action_index,
            reason,
            exit_code,
            output,
            output_truncated,
        } = interruption;
        self.mark_unit_changed(&unit_path);
        let course_id = self.course_id()?;
        let connection = &self.connection;
        let command = connection
            .query_row(
                "
                SELECT command
                FROM command_executions
                WHERE course_id = ?1 AND unit_path = ?2
                  AND step_id = ?3 AND action_index = ?4
                ",
                rusqlite::params![course_id, unit_path, step_id, action_index as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                CourseSessionError::WorkspaceReset(t("error.command_execution_not_found"))
            })?;
        let (output, output_truncated) =
            bound_command_output(output.unwrap_or_default(), output_truncated);
        let now = now_unix_millis() as i64;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "
            UPDATE command_executions
            SET status = 'paused', output = ?1, output_truncated = ?2,
                exit_code = ?3, interruption = ?4,
                updated_at = ?5
            WHERE course_id = ?6 AND unit_path = ?7
              AND step_id = ?8 AND action_index = ?9
            ",
            rusqlite::params![
                output.as_deref(),
                i64::from(output_truncated),
                exit_code,
                reason.as_str(),
                now,
                course_id,
                unit_path,
                step_id,
                action_index as i64,
            ],
        )?;
        transaction.execute(
            "
            UPDATE course_progress
            SET current_unit_path = ?1,
                current_step_id = ?2, updated_at = ?3
            WHERE id = ?4
            ",
            rusqlite::params![unit_path, step_id, now, course_id],
        )?;
        transaction.execute(
            "
            UPDATE unit_states
            SET status = 'paused', current_step_id = ?1,
                updated_at = ?2
            WHERE course_id = ?3 AND unit_path = ?4
            ",
            rusqlite::params![step_id, now, course_id, unit_path],
        )?;
        transaction.commit()?;
        if let Some(progress) = self.progress.as_mut() {
            progress.current_unit_path = Some(unit_path.clone());
            progress.current_step_id = Some(step_id.clone());
            progress.updated_at = now as u64;
            progress.recovery = Some(CommandRecoveryView {
                unit_path: unit_path.clone(),
                step_id: step_id.clone(),
                action_index,
                command,
                reason,
                output,
                output_truncated,
                exit_code,
            });
            if let Some(state) = progress
                .unit_states
                .iter_mut()
                .find(|state| state.unit_path == unit_path)
            {
                state.status = UnitStatus::Paused;
                state.current_step_id = Some(step_id.clone());
                state.updated_at = now as u64;
            }
        }
        self.running_command = false;
        Ok(())
    }

    pub fn complete_running_commands_except(
        &mut self,
        unit_path: &str,
        step_id: &str,
        action_index: usize,
    ) -> Result<(), CourseSessionError> {
        if self.progress.is_none() {
            self.running_command = false;
            return Ok(());
        }
        let now = now_unix_millis() as i64;
        // Skip the write entirely for Steps that never started a terminal
        // command; the flag is set by record_command_started.
        if self.running_command {
            let course_id = self.course_id()?;
            let connection = &self.connection;
            connection.execute(
                "
                UPDATE command_executions
                SET status = 'completed', updated_at = ?1
                WHERE course_id = ?2 AND status = 'running'
                  AND NOT (unit_path = ?3 AND step_id = ?4 AND action_index = ?5)
                ",
                rusqlite::params![now, course_id, unit_path, step_id, action_index as i64],
            )?;
            self.running_command = false;
        }
        if let Some(progress) = self.progress.as_mut() {
            if progress.recovery.as_ref().is_some_and(|recovery| {
                recovery.unit_path != unit_path
                    || recovery.step_id != step_id
                    || recovery.action_index != action_index
            }) {
                progress.recovery = None;
            }
            progress.updated_at = now as u64;
        }
        Ok(())
    }

    pub fn complete_all_running_commands(&mut self) -> Result<(), CourseSessionError> {
        if self.progress.is_none() {
            self.running_command = false;
            return Ok(());
        }
        let course_id = self.course_id()?;
        let now = now_unix_millis() as i64;
        let connection = &self.connection;
        connection.execute(
            "
            UPDATE command_executions
            SET status = 'completed', updated_at = ?1
            WHERE course_id = ?2 AND status = 'running'
            ",
            rusqlite::params![now, course_id],
        )?;
        self.running_command = false;
        if let Some(progress) = self.progress.as_mut() {
            progress.recovery = None;
            progress.updated_at = now as u64;
        }
        Ok(())
    }
}
