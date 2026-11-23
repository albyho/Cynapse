use super::*;

fn playback_step(loaded: &LoadedStep) -> PlaybackStep {
    PlaybackStep {
        parent_id: loaded.parent_id.clone(),
        parent_title: loaded.parent_title.clone(),
        unit_title: loaded.unit_title.clone(),
        unit_path: loaded.unit_path.clone(),
        step_id: loaded.step.step_id.clone(),
        step_title: loaded.step.title.clone(),
        requires_human: loaded.step.requires_human || loaded.step.challenge.is_some(),
        challenge: loaded.step.challenge.is_some(),
        actions: loaded
            .step
            .actions
            .iter()
            .enumerate()
            .map(|(index, action)| PlaybackAction {
                action: action.clone(),
                step_id: loaded.step.step_id.clone(),
                narration: loaded
                    .step
                    .action_narrations
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect(),
    }
}

impl Course {
    pub fn unit_by_path(&self, path: &str) -> Option<&UnitEntry> {
        self.units.iter().find(|entry| entry.path == path)
    }

    /// The Challenge declaration for one Step, if it is a challenge Step.
    pub fn challenge(&self, unit_path: &str, step_id: &str) -> Option<&Challenge> {
        self.steps
            .iter()
            .find(|step| step.unit_path == unit_path && step.step.step_id == step_id)?
            .step
            .challenge
            .as_ref()
    }

    /// Walk the Course Tree by Node Path. Unlike `unit_by_path`, this also
    /// finds grouping nodes, which carry a Directory Path but no Unit State.
    pub fn node_by_path(&self, path: &str) -> Option<&CourseNode> {
        if path.is_empty() {
            return None;
        }
        let mut node = &self.tree;
        for segment in path.split('/') {
            node = node.children.iter().find(|child| child.id == segment)?;
        }
        Some(node)
    }

    /// Directory Path for a Node Path (`dir_name` segments). Unknown paths fall
    /// back to the Node Path itself so callers keep a usable directory name.
    pub fn directory_path_for(&self, path: &str) -> Option<&str> {
        self.node_by_path(path).map(|node| node.dir_path.as_str())
    }

    pub fn workspace_path(&self) -> PathBuf {
        self.package_root.join(
            self.manifest
                .workspace
                .as_deref()
                .unwrap_or(super::DEFAULT_WORKSPACE),
        )
    }

    pub fn unit_workspace_path(&self, path: &str) -> Option<PathBuf> {
        self.unit_by_path(path)?;
        Some(
            path.split('/')
                .fold(self.workspace_path(), |workspace, segment| {
                    workspace.join(segment)
                }),
        )
    }

    /// 该 Unit 自己的 Seed 目录。目录名按 Directory Path 解析
    /// （`seeds/<dir path>/`）；Node Path 仍是该 Unit 的身份，用来定位
    /// `workspace/<node path>/`。不存在、或只作为后代 Seed 的路径容器
    /// （没有任何属于自己的文件）时，视为该 Unit 没有 Seed。
    /// 不再叠加根节点或祖先节点的 Seed——每个 Unit 的起始内容完全自足。
    pub fn workspace_seed_paths_for_path(&self, path: &str) -> Vec<PathBuf> {
        if self.unit_by_path(path).is_none() {
            return Vec::new();
        }
        let seed = self.workspace_seed_dir(path);
        let exclusions = self.workspace_seed_exclusions_for_path(path);
        if seed_has_own_files(&seed, &exclusions) {
            vec![seed]
        } else {
            Vec::new()
        }
    }

    pub fn workspace_seed_dir(&self, path: &str) -> PathBuf {
        let directory = self.directory_path_for(path).unwrap_or(path);
        self.package_root.join("seeds").join(directory)
    }

    /// 复制本 Unit 的 Seed 时要跳过的路径：Course Tree 中所有后代节点的
    /// Seed 根，以及父 Seed 根下与后代 Node Path 同名的条目。前者避免
    /// `seeds/<parent unit>/` 把物理嵌套的后代 Seed 复制进父 Workspace；后者
    /// 避免 `dir_name` 与 `id` 不同时，父 Seed 文件撞进后代 Unit Workspace。
    /// 按「所有后代节点」而非「所有后代 Unit」取，是因为分组节点同样只作为
    /// 路径容器，其下的文件也不属于父 Unit。
    pub fn workspace_seed_exclusions_for_path(&self, path: &str) -> HashSet<PathBuf> {
        fn collect(node: &CourseNode, current: &str, paths: &mut Vec<String>) {
            for child in &node.children {
                let child_path = if current.is_empty() {
                    child.id().to_string()
                } else {
                    format!("{current}/{}", child.id())
                };
                paths.push(child_path.clone());
                collect(child, &child_path, paths);
            }
        }

        let mut paths = Vec::new();
        collect(&self.tree, "", &mut paths);
        let prefix = format!("{path}/");
        let seed_root = self.workspace_seed_dir(path);
        let descendants = paths
            .into_iter()
            .filter(|descendant| descendant.starts_with(&prefix))
            .collect::<Vec<_>>();
        let mut exclusions = descendants
            .iter()
            .map(|descendant| self.workspace_seed_dir(descendant))
            .collect::<HashSet<_>>();
        // A descendant's Node Path can differ from its Directory Path. A parent
        // Seed entry named after the child's Node Path would otherwise be copied
        // into the child's workspace path, so exclude that collision as well.
        for descendant in &descendants {
            if let Some(relative) = descendant.strip_prefix(&prefix) {
                exclusions.insert(seed_root.join(relative));
            }
        }
        exclusions
    }

    /// 与 `workspace_seed_exclusions_for_path` 对应的「工作区」版本：返回 `<path>`
    /// 的所有后代 *Unit* 的工作区路径（绝对路径），用于在重置/清理本 Unit 工作区
    /// 时，把「未被本次重置的后代 Unit 工作区」（及其路径容器目录）排除在外，
    /// 避免重置父 Unit 时把已学过的子 Unit 磁盘工作区一并清掉。
    /// 分组节点不是 Unit，没有自己的工作区，但其目录可能作为后代 Unit 工作区的
    /// 路径容器而需要保留——`strip_prefix` 判断会自然覆盖这种祖先目录。
    pub fn descendant_unit_workspaces(&self, path: &str) -> Vec<(String, PathBuf)> {
        fn collect(node: &CourseNode, current: &str, paths: &mut Vec<String>) {
            for child in &node.children {
                let child_path = if current.is_empty() {
                    child.id().to_string()
                } else {
                    format!("{current}/{}", child.id())
                };
                paths.push(child_path.clone());
                collect(child, &child_path, paths);
            }
        }

        let mut paths = Vec::new();
        collect(&self.tree, "", &mut paths);
        let prefix = format!("{path}/");
        paths
            .into_iter()
            .filter(|descendant| descendant.starts_with(&prefix))
            .filter_map(|descendant| {
                self.unit_workspace_path(&descendant)
                    .map(|workspace| (descendant, workspace))
            })
            .collect()
    }

    pub fn resolve_resource_aliases_for_path(
        &self,
        value: &str,
        path: &str,
    ) -> Result<String, CourseError> {
        self.resolve_resource_aliases_for_path_with_root(
            value,
            path,
            &self.package_root.join("resources"),
        )
    }

    /// Resolves `{{resource:...}}` against an explicit resource root. The Client
    /// uses this with the Attempt's writable `workspace/resources/` copy instead
    /// of the authored package directory.
    pub fn resolve_resource_aliases_for_path_with_root(
        &self,
        value: &str,
        path: &str,
        resource_root: &Path,
    ) -> Result<String, CourseError> {
        if !value.contains("{{resource:") {
            return Ok(value.to_string());
        }
        if self.unit_by_path(path).is_none() {
            return Err(CourseError::Package(format!("unknown Node Path `{path}`")));
        }
        let directory_path = self
            .directory_path_for(path)
            .ok_or_else(|| CourseError::Package(format!("unknown Node Path `{path}`")))?
            .to_string();
        let canonical_root = resource_root
            .canonicalize()
            .unwrap_or_else(|_| resource_root.to_path_buf());
        let resources = resource_root.to_path_buf();
        let ancestor_path = directory_path
            .rsplit_once('/')
            .map(|(ancestor_path, _)| ancestor_path)
            .unwrap_or("");
        let ancestor_resources = if ancestor_path.is_empty() {
            resources.clone()
        } else {
            ancestor_path
                .split('/')
                .fold(resources.clone(), |resources, segment| {
                    resources.join(segment)
                })
        };
        let node_resources = directory_path
            .split('/')
            .fold(resources.clone(), |resources, segment| {
                resources.join(segment)
            });
        let resources = [
            ("course", resources),
            ("ancestor", ancestor_resources),
            ("node", node_resources),
        ];
        let mut resolved = value.to_string();
        for (scope, path) in resources {
            let token = format!("{{{{resource:{scope}}}}}");
            if !resolved.contains(&token) {
                continue;
            }
            let canonical = path.canonicalize().map_err(|error| {
                CourseError::Package(format!(
                    "resource scope `{scope}` is unavailable at {}: {error}",
                    path.display()
                ))
            })?;
            if !canonical.starts_with(&canonical_root) || !canonical.is_dir() {
                return Err(CourseError::Package(format!(
                    "resource scope `{scope}` must reference a directory inside the resource root"
                )));
            }
            let rendered = canonical.to_string_lossy();
            let rendered = rendered.strip_prefix(r"\\?\").unwrap_or(&rendered);
            resolved = resolved.replace(&token, rendered);
        }
        if resolved.contains("{{resource:") {
            return Err(CourseError::Package(format!(
                "command contains an unknown resource alias: {resolved}"
            )));
        }
        Ok(resolved)
    }

    pub fn playback_steps(&self) -> Vec<PlaybackStep> {
        self.steps.iter().map(playback_step).collect()
    }

    pub fn playback_step(&self, index: usize) -> Option<PlaybackStep> {
        self.steps.get(index).map(playback_step)
    }

    pub fn external_text_paths(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        for step in &self.steps {
            for action in &step.step.actions {
                let text_path = match action {
                    Action::InsertText(action) => action.text_path.as_deref(),
                    Action::ReplaceText(action) => action.text_path.as_deref(),
                    _ => None,
                };
                let Some(text_path) = text_path else {
                    continue;
                };
                let relative = PathBuf::from(text_path);
                if seen.insert(relative.clone()) {
                    files.push(relative);
                }
            }
        }
        files
    }

    pub fn audio_asset_paths(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        for entry in &self.units {
            for asset in &entry.unit.assets {
                let relative = PathBuf::from(&asset.path);
                if seen.insert(relative.clone()) {
                    files.push(relative);
                }
            }
        }
        files
    }

    pub fn action_narration(
        &self,
        unit_path: &str,
        step_id: &str,
        action_index: usize,
        phase: ActionNarrationPhase,
        clip_index: usize,
    ) -> Option<&ActionNarrationClip> {
        self.steps
            .iter()
            .find(|step| step.unit_path == unit_path && step.step.step_id == step_id)?
            .step
            .action_narrations
            .get(action_index)?
            .clips(phase)?
            .get(clip_index)
    }

    pub fn actions(&self) -> Vec<Action> {
        self.playback_steps()
            .into_iter()
            .flat_map(|step| step.actions.into_iter().map(|action| action.action))
            .collect()
    }

    pub fn unit_requires_app_adapter(&self, unit_path: &str) -> bool {
        self.steps
            .iter()
            .filter(|step| step.unit_path == unit_path)
            .flat_map(|step| &step.step.actions)
            .any(|action| !action.is_client_local())
    }

    pub fn unit_has_invalid_adapter_configuration(&self, unit_path: &str) -> bool {
        self.unit_by_path(unit_path).is_some_and(|entry| {
            entry.unit.adapters.is_empty()
                && (self.unit_requires_app_adapter(unit_path) || !entry.unit.environment.is_empty())
        })
    }

    pub fn environment_requirements(&self) -> &[EnvironmentRequirement] {
        &self.tree.environment
    }

    pub fn effective_adapters_for_path(&self, path: &str) -> Vec<AdapterId> {
        self.unit_by_path(path)
            .map(|entry| entry.unit.adapters.clone())
            .unwrap_or_default()
    }

    pub fn effective_environment_requirements_for_path(
        &self,
        path: &str,
    ) -> Vec<EnvironmentRequirement> {
        self.unit_by_path(path)
            .map(|entry| entry.unit.environment.clone())
            .unwrap_or_default()
    }
}

/// Seed 目录是否存在「归属于本 Unit」的文件。后代节点的 Seed 目录整棵跳过，
/// 因此一个纯粹用于承载后代 Seed 的目录会被判定为没有 Seed，
/// 不会让 Unit 凭空得到一个空的工作区目录。
fn seed_has_own_files(directory: &Path, exclusions: &HashSet<PathBuf>) -> bool {
    fn walk(directory: &Path, exclusions: &HashSet<PathBuf>) -> bool {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return false;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                if exclusions.contains(&path) {
                    continue;
                }
                if walk(&path, exclusions) {
                    return true;
                }
            } else if file_type.is_file() {
                return true;
            }
        }
        false
    }

    directory.is_dir() && walk(directory, exclusions)
}
