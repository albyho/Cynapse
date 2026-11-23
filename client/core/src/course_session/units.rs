use super::store::clear_resume_action;
use super::*;

/// Unit start/selection, reset scopes, and workspace preparation.
impl CourseSession {
    pub fn start_first_unit(
        &mut self,
        confirmed: bool,
    ) -> Result<CourseSessionState, CourseSessionError> {
        let unit_path = self
            .active_course
            .as_ref()
            .and_then(|course| course.units.first())
            .map(|entry| entry.path.clone())
            .ok_or_else(|| CourseSessionError::NoActiveUnit)?;
        self.start_unit_with_removals_at(&unit_path, confirmed, true)
            .map(|outcome| outcome.state)
    }

    pub fn workspace_reset_paths_for_course_reset(
        &self,
        confirmed: bool,
    ) -> Result<Vec<String>, CourseSessionError> {
        if !confirmed {
            return Err(CourseSessionError::ConfirmationRequired);
        }
        self.workspace_reset_paths_for_course()
    }

    pub fn reset_scope_for_unit(
        &self,
        unit_path: &str,
    ) -> Result<UnitResetScope, CourseSessionError> {
        let plan = self.unit_reset_plan(unit_path)?;
        let exclude_paths = if self.active_workspace().is_some() {
            self.active_course
                .as_ref()
                .map(|course| {
                    course
                        .descendant_unit_workspaces(unit_path)
                        .into_iter()
                        .map(|(path, _)| path)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(UnitResetScope {
            workspace_paths: if self.active_workspace().is_some() {
                plan.reset_workspace_paths
            } else {
                Vec::new()
            },
            exclude_paths,
        })
    }

    fn unit_reset_plan(&self, unit_path: &str) -> Result<UnitResetPlan, CourseSessionError> {
        let course = self
            .active_course
            .as_ref()
            .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;
        course
            .unit_by_path(unit_path)
            .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;

        Ok(UnitResetPlan {
            affected_unit_paths: vec![unit_path.to_string()],
            reset_workspace_paths: vec![unit_path.to_string()],
        })
    }

    pub fn reset_unit(
        &mut self,
        unit_path: &str,
        confirmed: bool,
    ) -> Result<CourseSessionOutcome, CourseSessionError> {
        if !confirmed {
            return Err(CourseSessionError::ConfirmationRequired);
        }
        let plan = self.unit_reset_plan(unit_path)?;
        for path in plan.affected_unit_paths.clone() {
            self.mark_unit_changed(&path);
        }
        let progress = self
            .reload_active_progress()?
            .ok_or(CourseSessionError::NoActiveProgress)?;
        if !unit_has_recorded_progress(Some(&progress), unit_path) {
            return Err(CourseSessionError::UnitResetNotStarted(
                unit_path.to_string(),
            ));
        }
        let (course_id, current_steps, removed_workspace_paths) = {
            let course = self
                .active_course
                .as_ref()
                .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;
            let current_steps = course
                .steps
                .iter()
                .filter(|step| step.unit_path == unit_path)
                .map(|step| (step.unit_path.clone(), step.step.step_id.clone()))
                .collect::<Vec<_>>();
            let removed_workspace_paths = plan
                .reset_workspace_paths
                .iter()
                .filter(|path| {
                    course
                        .unit_workspace_path(path)
                        .is_some_and(|workspace| workspace.exists())
                })
                .cloned()
                .collect::<Vec<_>>();
            (
                course.manifest.id.clone(),
                current_steps,
                removed_workspace_paths,
            )
        };

        // Reset is deliberately fail-fast: if workspace cleanup or the database
        // write fails, keep the partial state and surface the error to the user.
        self.prepare_unit_workspace(unit_path, true, false, false)?;
        let now = now_unix_millis() as i64;
        let clear_current_position = progress
            .current_unit_path
            .as_ref()
            .is_some_and(|current| plan.affected_unit_paths.contains(current));
        let connection = &self.connection;
        reset_unit_progress(
            connection,
            UnitProgressReset {
                course_id: &course_id,
                clear_current_position,
                affected_unit_paths: &plan.affected_unit_paths,
                current_steps: &current_steps,
                now,
            },
        )?;
        self.running_command = false;
        self.reload_progress()?;
        // Reset never derives selection from the resume cursor. Only the reset
        // Unit loses its selection; another explicit selection is preserved.
        if self.selected_unit_path.as_deref() == Some(unit_path) {
            self.selected_unit_path = None;
        }
        persist_selection(&self.connection, self.selected_unit_path.as_deref())?;
        Ok(CourseSessionOutcome {
            state: self.state(),
            removed_workspace_paths,
        })
    }

    pub fn workspace_reset_paths_for_course(&self) -> Result<Vec<String>, CourseSessionError> {
        if self.active_path.is_none() {
            return Err(CourseSessionError::WorkspaceReset(
                "no active course".into(),
            ));
        }
        Ok(if self.active_workspace().is_some() {
            vec![".".to_string()]
        } else {
            Vec::new()
        })
    }

    pub(super) fn unit_completed(&self, unit_path: &str) -> bool {
        let Some(course) = self.active_course.as_ref() else {
            return false;
        };
        let progress_index = self.progress.as_ref().map(ProgressIndex::new);
        let steps = course
            .steps
            .iter()
            .filter(|loaded| loaded.unit_path == unit_path);
        unit_is_completed(steps, progress_index.as_ref(), unit_path)
    }

    pub fn start_unit(
        &mut self,
        unit_path: &str,
        confirmed: bool,
    ) -> Result<CourseSessionState, CourseSessionError> {
        self.start_unit_with_removals_at(unit_path, confirmed, true)
            .map(|outcome| outcome.state)
    }

    pub fn select_unit(
        &mut self,
        unit_path: &str,
    ) -> Result<CourseSessionState, CourseSessionError> {
        self.start_unit_with_removals_at(unit_path, false, false)
            .map(|outcome| outcome.state)
    }

    /// Deselects the Unit and clears the course cursor, keeping Step progress.
    pub fn clear_selection(&mut self) -> Result<CourseSessionState, CourseSessionError> {
        let course_id = self.course_id()?;
        let now = now_unix_millis() as i64;
        let connection = &self.connection;
        let transaction = connection.unchecked_transaction()?;
        clear_resume_cursor(&transaction, &course_id, now)?;
        persist_selection(&transaction, None)?;
        transaction.commit()?;
        if let Some(progress) = self.progress.as_mut() {
            progress.current_unit_path = None;
            progress.current_step_id = None;
            progress.updated_at = now as u64;
        }
        self.selected_unit_path = None;
        Ok(self.state())
    }

    fn start_unit_with_removals_at(
        &mut self,
        unit_path: &str,
        confirmed: bool,
        prepare_workspace: bool,
    ) -> Result<CourseSessionOutcome, CourseSessionError> {
        self.reload_progress()?;
        let course = self
            .active_course
            .as_ref()
            .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;
        course
            .unit_by_path(unit_path)
            .ok_or_else(|| CourseSessionError::UnitNotFound(unit_path.to_string()))?;
        if !confirmed && self.unit_completed(unit_path) {
            return Err(CourseSessionError::ConfirmationRequired);
        }
        self.selected_unit_path = Some(unit_path.to_string());
        let target_completed = self.unit_completed(unit_path);
        let existing = self
            .progress
            .as_ref()
            .map(|progress| unit_was_reset(progress, unit_path));
        // Starting after traversal is not a new course round: only the target
        // Unit may be reset, and only when it was already completed.
        let (replace_workspace, reset_marker) = match existing {
            Some(reset) if !target_completed => (reset, reset),
            Some(_) => {
                if !confirmed {
                    return Err(CourseSessionError::ConfirmationRequired);
                }
                (true, true)
            }
            None => {
                self.ensure_progress()?;
                (true, false)
            }
        };
        if prepare_workspace {
            self.ensure_active_resources()?;
            let create_seedless_workspace = self.unit_needs_workspace(unit_path);
            // 开始 Unit 才是 Seed 的唯一落地点。
            self.prepare_unit_workspace(
                unit_path,
                replace_workspace,
                create_seedless_workspace,
                true,
            )?;
        }
        if reset_marker {
            self.clear_unit_step_rows(unit_path)?;
            let course_id = self.course_id()?;
            clear_resume_action(&self.connection, &course_id)?;
        }
        self.set_unit_position(unit_path)?;
        self.prepared_playback_steps()?;
        let connection = &self.connection;
        persist_selection(connection, self.selected_unit_path.as_deref())?;
        Ok(CourseSessionOutcome {
            state: self.state(),
            removed_workspace_paths: Vec::new(),
        })
    }

    fn unit_needs_workspace(&self, unit_path: &str) -> bool {
        self.active_course.as_ref().is_some_and(|course| {
            course.steps.iter().any(|loaded| {
                loaded.unit_path == unit_path
                    && loaded
                        .step
                        .actions
                        .iter()
                        .any(|action| !action.is_client_local())
            })
        })
    }

    pub(super) fn prepare_unit_workspace(
        &self,
        unit_path: &str,
        replace: bool,
        create_seedless_workspace: bool,
        restore_seed: bool,
    ) -> Result<(), CourseSessionError> {
        let course = self
            .active_course
            .as_ref()
            .ok_or_else(|| CourseSessionError::WorkspaceReset("no active course".into()))?;
        let Some(target) = course.unit_workspace_path(unit_path) else {
            return Ok(());
        };
        let workspace_root = course.workspace_path();
        super::validate_workspace_path(&workspace_root, &target)?;
        // 重置路径只负责把工作区清回“尚未开始”的空状态，不落任何 Seed。
        // 三步走的第二步依然存在（刷新快照 + 准备工作区），只是准备的是空状态；
        // Seed 统一由下一次 start_unit 复制。
        if !restore_seed {
            if target.exists() {
                // 就地清理本 Unit 自有内容，排除未被重置的后代 Unit 工作区，
                // 避免重置父 Unit 时连子 Unit（已学过）的磁盘工作区一起清掉。
                let descendants = course
                    .descendant_unit_workspaces(unit_path)
                    .into_iter()
                    .map(|(_, workspace)| workspace)
                    .collect::<Vec<_>>();
                clear_own_workspace_entries(&target, &descendants)?;
                // Empty directories are harmless; a transient Windows lock must
                // not turn Unit Reset into a five-second wait.
                let _ = remove_directory_if_empty(&target);
            }
            if let Some(parent) = target.parent()
                && workspace_root.as_path() != parent
                && let Err(error) = remove_directory_if_empty(parent)
                && !is_workspace_path_lock(&error)
            {
                return Err(error);
            }
            return Ok(());
        }
        if target.is_dir() && !replace {
            // 工作区已经存在：只补本 Unit Seed 里新增、工作区还没有的文件，
            // 已有文件（含学员的改动）一律不动；后代 Unit 的 Seed 目录整棵跳过。
            let seeds = course.workspace_seed_paths_for_path(unit_path);
            if !seeds.is_empty() {
                let exclusions = course.workspace_seed_exclusions_for_path(unit_path);
                super::top_up_missing_seed_files(&seeds, &target, &exclusions)?;
            }
            return Ok(());
        }
        let seeds = course.workspace_seed_paths_for_path(unit_path);
        let exclusions = course.workspace_seed_exclusions_for_path(unit_path);
        let descendants = course
            .descendant_unit_workspaces(unit_path)
            .into_iter()
            .map(|(_, workspace)| workspace)
            .collect::<Vec<_>>();
        // 父 Unit 的目录同时也是 Descendant Unit Workspace 的容器，不能整目录删除。
        // 先清掉父 Unit 自己的条目，再原地补齐 Seed；子工作区始终留在原位。
        if target.exists() {
            clear_own_workspace_entries(&target, &descendants)?;
        }
        // 没有 Seed、且 Unit 不需要空工作区时，工作区应保持“未创建”的初始状态。
        // 这与 `replace` 无关：重新开始时进度仍在、`replace` 为 false，
        // 若目录被删掉同样不该凭空造出一个空目录。
        if seeds.is_empty() && !create_seedless_workspace {
            if target.exists() {
                let _ = remove_directory_if_empty(&target);
            }
            if let Some(parent) = target.parent()
                && workspace_root.as_path() != parent
                && let Err(error) = remove_directory_if_empty(parent)
                && !is_workspace_path_lock(&error)
            {
                return Err(error);
            }
            return Ok(());
        }
        std::fs::create_dir_all(&target)?;
        for seed in seeds {
            copy_directory_overlay(&seed, &target, &exclusions)?;
        }
        Ok(())
    }
}
