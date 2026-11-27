use super::*;

pub(super) fn apply_fake_file_system(
    workspace: &Path,
    action: &Action,
) -> Result<(), ScenarioError> {
    match action {
        Action::Present | Action::Wait(_) => {}
        Action::OpenFile(_) => {}
        // 预览是纯展示，headless 场景不开面板，也不碰工作区。
        Action::OpenPreview(_) => {}
        // 源码 + 预览同样是纯展示。
        Action::OpenFileWithPreview(_) => {}
        // 关闭 tab 是纯展示，headless 场景没有 tab 可关。
        Action::CloseFile(_) | Action::ClosePreview(_) => {}
        // 定位是纯展示，headless 场景不移动视口。
        Action::RevealText(_) => {}
        // 清除定位高亮同样是纯展示。
        Action::ClearReveal => {}
        Action::SetCursor(_) => {}
        Action::InsertText(action) => {
            let path = safe_workspace_path(workspace, &action.path)?;
            let current = fs::read_to_string(&path)?;
            let updated = cynapse_course::apply_insert_text(&current, action)
                .map_err(|error| ScenarioError::Protocol(error.to_string()))?;
            fs::write(path, updated)?;
        }
        Action::DeleteText(action) => {
            let path = safe_workspace_path(workspace, &action.path)?;
            let current = fs::read_to_string(&path)?;
            let updated = cynapse_course::apply_delete_text(&current, action)
                .map_err(|error| ScenarioError::Protocol(error.to_string()))?;
            fs::write(path, updated)?;
        }
        Action::ReplaceText(action) => {
            let path = safe_workspace_path(workspace, &action.path)?;
            let current = fs::read_to_string(&path)?;
            let updated = cynapse_course::apply_replace_text(&current, action)
                .map_err(|error| ScenarioError::Protocol(error.to_string()))?;
            fs::write(path, updated)?;
        }
        Action::CreateDirectory(inner) => {
            let path = safe_workspace_path(workspace, &inner.path)?;
            let plan = file_action_plan(&path, action)?;
            match plan {
                cynapse_course::FileActionPlan::CreateDirectory { create_parents } => {
                    if create_parents {
                        fs::create_dir_all(&path)?;
                    } else if !path.is_dir() {
                        fs::create_dir(&path)?;
                    }
                }
                plan => return Err(unsupported_plan(plan)),
            }
        }
        Action::CreateFile(inner) => {
            let path = safe_workspace_path(workspace, &inner.path)?;
            let plan = file_action_plan(&path, action)?;
            match plan {
                cynapse_course::FileActionPlan::CreateFile { create_parents } => {
                    if create_parents && let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)?;
                }
                plan => return Err(unsupported_plan(plan)),
            }
        }
        Action::DeleteFile(inner) => {
            let path = safe_workspace_path(workspace, &inner.path)?;
            let plan = file_action_plan(&path, action)?;
            match plan {
                cynapse_course::FileActionPlan::DeleteFile => fs::remove_file(path)?,
                cynapse_course::FileActionPlan::SkipMissing => {}
                plan => return Err(unsupported_plan(plan)),
            }
        }
        Action::DeleteDirectory(inner) => {
            let path = safe_workspace_path(workspace, &inner.path)?;
            let plan = file_action_plan(&path, action)?;
            match plan {
                cynapse_course::FileActionPlan::DeleteDirectory { recursive } => {
                    if recursive {
                        fs::remove_dir_all(path)?;
                    } else {
                        fs::remove_dir(path)?;
                    }
                }
                cynapse_course::FileActionPlan::SkipMissing => {}
                plan => return Err(unsupported_plan(plan)),
            }
        }
        Action::TerminalEnsure(_) | Action::TerminalCommand(_) => {}
        Action::JupyterLabOpenNotebook(_)
        | Action::JupyterLabCellSelect(_)
        | Action::JupyterLabCellRun(_)
        | Action::JupyterLabRestartKernel(_)
        | Action::JupyterLabInterruptKernel(_)
        | Action::JupyterLabOpenPreview(_)
        | Action::JupyterLabCloseDocument(_)
        | Action::JupyterLabCellInsertText(_)
        | Action::JupyterLabCellDeleteText(_)
        | Action::JupyterLabCellReplaceText(_)
        | Action::JupyterLabCellInsert(_)
        | Action::JupyterLabCellMove(_)
        | Action::JupyterLabCellDuplicate(_)
        | Action::JupyterLabCellDelete(_) => {}
    }
    Ok(())
}

fn file_action_plan(
    path: &Path,
    action: &Action,
) -> Result<cynapse_course::FileActionPlan, ScenarioError> {
    let target = target_state(path)?;
    let parent = match path.parent() {
        Some(parent) => target_state(parent)?,
        None => cynapse_course::TargetState::Directory,
    };
    let directory_empty = if target == cynapse_course::TargetState::Directory {
        fs::read_dir(path)?.next().is_none()
    } else {
        true
    };
    cynapse_course::plan_file_action(
        action,
        cynapse_course::TargetFacts {
            target,
            parent,
            directory_empty,
        },
    )
    .map_err(|error| {
        ScenarioError::Protocol(format!(
            "{} for {path:?}",
            match error {
                cynapse_course::FileActionError::TargetIsDirectory => "target is a directory",
                cynapse_course::FileActionError::TargetIsFile => "target is a file",
                cynapse_course::FileActionError::TargetMissing => "target does not exist",
                cynapse_course::FileActionError::ParentMissing => "parent directory does not exist",
                cynapse_course::FileActionError::ParentNotDirectory => "parent is not a directory",
                cynapse_course::FileActionError::DirectoryNotEmpty =>
                    "directory is not empty; set recursive",
                cynapse_course::FileActionError::NotAFileAction => "not a file action",
            }
        ))
    })
}

fn target_state(path: &Path) -> Result<cynapse_course::TargetState, ScenarioError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(cynapse_course::TargetState::Directory),
        Ok(_) => Ok(cynapse_course::TargetState::File),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(cynapse_course::TargetState::Missing)
        }
        Err(error) => Err(error.into()),
    }
}

fn unsupported_plan(plan: cynapse_course::FileActionPlan) -> ScenarioError {
    ScenarioError::Protocol(format!("unexpected file action plan: {plan:?}"))
}

pub(super) fn safe_workspace_path(
    workspace: &Path,
    relative: &str,
) -> Result<PathBuf, ScenarioError> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(ScenarioError::Protocol(format!(
            "action path escapes workspace: {relative:?}"
        )));
    }
    Ok(workspace.join(relative))
}

pub(super) fn list_workspace_files(workspace: &Path) -> Result<Vec<String>, ScenarioError> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<String>) -> std::io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                visit(root, &path, files)?;
            } else if let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(workspace, workspace, &mut files)?;
    files.sort();
    Ok(files)
}

const MAX_WORKSPACE_CONTENT_FILE_BYTES: u64 = 1024 * 1024;
const MAX_WORKSPACE_CONTENTS_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn capture_workspace_contents(
    workspace: &Path,
    files: &[String],
) -> (std::collections::BTreeMap<String, String>, Vec<String>) {
    capture_workspace_contents_with_limits(
        workspace,
        files,
        MAX_WORKSPACE_CONTENT_FILE_BYTES,
        MAX_WORKSPACE_CONTENTS_BYTES,
    )
}

fn capture_workspace_contents_with_limits(
    workspace: &Path,
    files: &[String],
    max_file_bytes: u64,
    max_total_bytes: usize,
) -> (std::collections::BTreeMap<String, String>, Vec<String>) {
    let mut contents = std::collections::BTreeMap::new();
    let mut skipped = Vec::new();
    let mut total_bytes = 0usize;
    for relative in files {
        // Runtime resources are a derived read-only copy of package resources.
        // Listing them in workspace_files is enough; copying their text into
        // every ScenarioReport makes large courses prohibitively expensive.
        if relative == "resources" || relative.starts_with("resources/") {
            continue;
        }
        let Ok(metadata) = fs::metadata(workspace.join(relative)) else {
            continue;
        };
        if metadata.len() > max_file_bytes {
            skipped.push(relative.clone());
            continue;
        }
        let Ok(text) = fs::read_to_string(workspace.join(relative)) else {
            continue;
        };
        if total_bytes.saturating_add(text.len()) > max_total_bytes {
            skipped.push(relative.clone());
            continue;
        }
        total_bytes += text.len();
        contents.insert(relative.clone(), text);
    }
    skipped.sort();
    (contents, skipped)
}

pub(super) fn copy_directory(source: &Path, destination: &Path) -> Result<(), ScenarioError> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_contents_skip_resources_and_respect_limits() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path();
        std::fs::create_dir_all(workspace.join("resources")).unwrap();
        std::fs::write(workspace.join("a.txt"), "abc").unwrap();
        std::fs::write(workspace.join("b.txt"), "de").unwrap();
        std::fs::write(workspace.join("c.txt"), "fgh").unwrap();
        std::fs::write(workspace.join("big.txt"), "12345").unwrap();
        std::fs::write(workspace.join("resources/data.txt"), "res").unwrap();
        let files = vec![
            "a.txt".to_string(),
            "b.txt".to_string(),
            "c.txt".to_string(),
            "big.txt".to_string(),
            "resources/data.txt".to_string(),
        ];

        let (contents, skipped) = capture_workspace_contents_with_limits(workspace, &files, 4, 6);

        assert_eq!(contents.get("a.txt").map(String::as_str), Some("abc"));
        assert_eq!(contents.get("b.txt").map(String::as_str), Some("de"));
        assert!(!contents.contains_key("c.txt"));
        assert!(!contents.contains_key("big.txt"));
        assert!(!contents.contains_key("resources/data.txt"));
        assert_eq!(skipped, vec!["big.txt", "c.txt"]);
    }

    #[test]
    fn safe_workspace_path_rejects_escaping_paths() {
        let workspace = Path::new("workspace");
        for path in ["../outside", "../../outside", "/outside"] {
            assert!(
                safe_workspace_path(workspace, path).is_err(),
                "expected `{path}` to be rejected"
            );
        }
        #[cfg(windows)]
        for path in ["C:outside", r"\outside"] {
            assert!(
                safe_workspace_path(workspace, path).is_err(),
                "expected Windows escape `{path}` to be rejected"
            );
        }
        assert!(safe_workspace_path(workspace, "ch/main").is_ok());
    }
}
