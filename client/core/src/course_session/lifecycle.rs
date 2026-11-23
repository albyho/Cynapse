use super::store::clear_resume_action;
use super::*;
use cynapse_client_i18n::t;

/// Course open, restore, recent list, and course reset.
impl CourseSession {
    pub fn open_course(
        &mut self,
        package_path: impl AsRef<Path>,
    ) -> Result<CourseSessionState, CourseSessionError> {
        #[cfg(test)]
        let fail_open = self.test_hooks.fail_next_open.take();
        let package_path = normalize_package_path(package_path.as_ref())?;
        let course = load_package(&package_path)?;
        ensure_course_workspace(&course)?;
        let active_workspace_path = course.workspace_path();
        let path_string = package_path.to_string_lossy().to_string();

        // Stage everything first. A failed open must not leave the session or
        // the persisted active-course pointer ahead of the reducer/UI.
        let connection = &self.connection;
        let mut progress = load_progress(connection, &course.manifest.id)?;
        let clear_versioned_progress = progress
            .as_ref()
            .is_some_and(|progress| progress.version != course.manifest.version);
        if clear_versioned_progress {
            progress = None;
        }
        // Selection is explicit and stored separately from the resume cursor.
        // A stored selection is only valid for the course that stored it.
        let stored_course_path = setting(connection, "active_course_path")?;
        let stored_selection = if stored_course_path.as_deref() == Some(path_string.as_str()) {
            setting(connection, "active_unit_path")?
        } else {
            None
        };
        let mut selected_unit_path = if clear_versioned_progress {
            None
        } else {
            stored_selection
                .filter(|unit_path| course.units.iter().any(|entry| &entry.path == unit_path))
        };
        if progress.is_some() {
            #[cfg(test)]
            if fail_open == Some(LifecycleFailpoint::Resources) {
                return Err(CourseSessionError::WorkspaceReset(
                    "injected resource preparation failure".to_string(),
                ));
            }
            ensure_course_resources(&package_path, active_workspace_path.as_path())?;
        }
        if selected_unit_path.is_none() && progress.is_none() {
            selected_unit_path = course.units.first().map(|entry| entry.path.clone());
        }

        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "
            INSERT INTO recent_courses(path, id, title, last_opened_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(path) DO UPDATE SET
                id = excluded.id,
                title = excluded.title,
                last_opened_at = excluded.last_opened_at
            ",
            rusqlite::params![
                path_string,
                course.manifest.id,
                course.manifest.title,
                now_unix_millis() as i64,
            ],
        )?;
        transaction.execute(
            "
            INSERT INTO settings(key, value)
            VALUES ('active_course_path', ?1)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            ",
            [&path_string],
        )?;
        if clear_versioned_progress {
            transaction.execute(
                "DELETE FROM course_progress WHERE id = ?1",
                [&course.manifest.id],
            )?;
            clear_resume_action(&transaction, &course.manifest.id)?;
        }
        persist_selection(&transaction, selected_unit_path.as_deref())?;
        let recent_courses = load_recent_courses(&transaction)?;
        #[cfg(test)]
        if fail_open == Some(LifecycleFailpoint::Commit) {
            return Err(CourseSessionError::WorkspaceReset(
                "injected open commit failure".to_string(),
            ));
        }
        transaction.commit()?;

        self.set_active_course(course);
        self.active_path = Some(path_string);
        self.active_workspace_path = Some(active_workspace_path);
        self.selected_unit_path = selected_unit_path;
        self.progress = progress;
        self.running_command = false;
        self.recent_courses = recent_courses;
        Ok(self.state())
    }

    /// Clears recent-course history without changing the active Course.
    pub fn clear_recent_courses(&mut self) -> Result<CourseSessionState, CourseSessionError> {
        self.connection.execute("DELETE FROM recent_courses", [])?;
        self.recent_courses.clear();
        Ok(self.state())
    }

    /// Clears the active Course and its persisted selection without deleting
    /// progress, recent-course history, or Unit workspaces.
    pub fn close_course(&mut self) -> Result<CourseSessionState, CourseSessionError> {
        let connection = &self.connection;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM settings WHERE key IN ('active_course_path', 'active_unit_path')",
            [],
        )?;
        transaction.commit()?;
        self.clear_active_course_in_memory();
        Ok(self.state())
    }

    pub fn reset_course(
        &mut self,
        confirmed: bool,
    ) -> Result<CourseSessionState, CourseSessionError> {
        self.reset_course_with_removals(confirmed)
            .map(|outcome| outcome.state)
    }

    pub fn reset_course_with_removals(
        &mut self,
        confirmed: bool,
    ) -> Result<CourseSessionOutcome, CourseSessionError> {
        if !confirmed {
            return Err(CourseSessionError::ConfirmationRequired);
        }
        let course_path = self
            .active_path
            .clone()
            .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
        let course = load_package(Path::new(&course_path))?;
        let unit_paths = course
            .units
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let removed_workspace_paths = unit_paths
            .iter()
            .filter(|unit_path| {
                course
                    .unit_workspace_path(unit_path)
                    .is_some_and(|workspace| workspace.exists())
            })
            .cloned()
            .collect::<Vec<_>>();
        let first_unit_path = course
            .units
            .first()
            .map(|entry| entry.path.clone())
            .ok_or_else(|| CourseSessionError::WorkspaceReset("course has no leaves".into()))?;
        let progress = self
            .reload_active_progress()?
            .ok_or(CourseSessionError::NoActiveProgress)?;
        let course_id = course.manifest.id.clone();
        let _ = progress;
        let workspace = self.active_workspace_path.clone().ok_or_else(|| {
            CourseSessionError::WorkspaceReset(t("error.course_workspace_missing"))
        })?;
        // Course Reset is intentionally not transactional: close terminals/files first
        // in the caller, then clear the whole workspace. A mid-operation failure can
        // leave a partially cleared workspace; the caller reports the error and the
        // learner may retry. No rollback is attempted.
        super::clear_course_workspace(&workspace)?;
        let now = now_unix_millis() as i64;
        let connection = &self.connection;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM command_executions WHERE course_id = ?1",
            [&course_id],
        )?;
        transaction.execute("DELETE FROM step_states WHERE course_id = ?1", [&course_id])?;
        transaction.execute(
            "UPDATE unit_states
             SET status = 'not_started', current_step_id = NULL, updated_at = ?1
             WHERE course_id = ?2",
            rusqlite::params![now, course_id],
        )?;
        clear_resume_cursor(&transaction, &course_id, now)?;
        transaction.commit()?;
        self.running_command = false;
        self.set_active_course(course);
        self.selected_unit_path = Some(first_unit_path.clone());
        self.reload_progress()?;
        persist_selection(&self.connection, Some(&first_unit_path))?;
        Ok(CourseSessionOutcome {
            state: self.state(),
            removed_workspace_paths,
        })
    }

    pub(super) fn restore(&mut self) -> Result<(), CourseSessionError> {
        #[cfg(test)]
        let fail_restore = self.test_hooks.fail_next_restore.take();
        let connection = &self.connection;
        let recent_courses = load_recent_courses(connection)?;
        let active_path: Option<String> = connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'active_course_path'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let stored_selected_unit_path = setting(connection, "active_unit_path")?;

        let Some(path) = active_path else {
            self.recent_courses = recent_courses;
            self.active_path = None;
            self.active_workspace_path = None;
            self.clear_active_course();
            self.selected_unit_path = None;
            self.progress = None;
            return Ok(());
        };
        let course = match load_package(Path::new(&path)) {
            Ok(course) => course,
            Err(_) => {
                connection.execute("DELETE FROM settings WHERE key = 'active_course_path'", [])?;
                self.recent_courses = recent_courses;
                self.active_path = None;
                self.active_workspace_path = None;
                self.clear_active_course();
                self.selected_unit_path = None;
                self.progress = None;
                return Ok(());
            }
        };
        ensure_course_workspace(&course)?;
        let workspace = course.workspace_path();
        let mut progress = load_progress(connection, &course.manifest.id)?;
        let clear_versioned = progress
            .as_ref()
            .is_some_and(|progress| progress.version != course.manifest.version);
        if clear_versioned {
            progress = None;
        }
        let mut selected_unit_path = if clear_versioned {
            None
        } else {
            stored_selected_unit_path
        };
        if !selected_unit_path
            .as_deref()
            .is_some_and(|unit_path| course.units.iter().any(|entry| entry.path == unit_path))
        {
            selected_unit_path = None;
        }
        if selected_unit_path.is_none() && progress.is_none() {
            selected_unit_path = course.units.first().map(|entry| entry.path.clone());
        }
        if progress.is_some() {
            #[cfg(test)]
            if fail_restore == Some(LifecycleFailpoint::Resources) {
                return Err(CourseSessionError::WorkspaceReset(
                    "injected restore resource preparation failure".to_string(),
                ));
            }
            ensure_course_resources(Path::new(&path), workspace.as_path())?;
        }

        let transaction = connection.unchecked_transaction()?;
        if clear_versioned {
            transaction.execute(
                "DELETE FROM course_progress WHERE id = ?1",
                [&course.manifest.id],
            )?;
            clear_resume_action(&transaction, &course.manifest.id)?;
            persist_selection(&transaction, selected_unit_path.as_deref())?;
        }
        #[cfg(test)]
        if fail_restore == Some(LifecycleFailpoint::Commit) {
            return Err(CourseSessionError::WorkspaceReset(
                "injected restore commit failure".to_string(),
            ));
        }
        transaction.commit()?;

        self.recent_courses = recent_courses;
        self.active_path = Some(path);
        self.active_workspace_path = Some(workspace);
        self.set_active_course(course);
        self.selected_unit_path = selected_unit_path;
        self.progress = progress;
        self.running_command = false;
        Ok(())
    }
}
