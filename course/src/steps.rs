use super::*;

/// Inline `text_path` files are snippets, not videos; refuse to read them all
/// into memory without a bound.
const MAX_TEXT_PATH_BYTES: u64 = 8 * 1024 * 1024;
/// The Client's adapter acknowledgement deadline; keep course-declared
/// timeouts below it so a valid package cannot hit the server watchdog.
const MAX_ACTION_TIMEOUT_MS: u64 = 30 * 60 * 1000;

pub(super) fn load_step(
    root: &Path,
    canonical_root: &Path,
    reference: &StepReference,
    unit_audio_assets: &HashMap<String, action_narration::ResolvedAudioAsset>,
) -> Result<(PathBuf, Step), CourseError> {
    let relative = validate_step_file_path(&reference.step_file)?;
    let path = root.join(&relative);
    let canonical_path = path.canonicalize()?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(CourseError::Package(format!(
            "step_file `{}` is outside the Course Package",
            reference.step_file
        )));
    }
    let step_value: serde_json::Value = serde_json::from_slice(&std::fs::read(&canonical_path)?)?;
    let step = parse_step_value(
        root,
        canonical_root,
        &relative,
        step_value,
        unit_audio_assets,
    )?;
    Ok((relative, step))
}

/// Check that a Step file reference is a package-relative `.json` file.
///
/// This is deliberately lexical: a new Step file does not exist yet, so
/// `canonicalize` runs only in [`load_step`].
pub(super) fn validate_step_file_path(reference: &str) -> Result<PathBuf, CourseError> {
    if reference.contains('\\') {
        return Err(CourseError::Package(format!(
            "step_file `{reference}` must use forward slashes"
        )));
    }
    let relative = Path::new(reference);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CourseError::Package(format!(
            "step_file `{reference}` is outside the Course Package"
        )));
    }
    if relative
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("json")
    {
        return Err(CourseError::Package(format!(
            "step_file `{reference}` must use the .json extension"
        )));
    }
    Ok(relative.to_path_buf())
}

/// Parse a Step document into the runtime model.
///
/// The bytes normally come from `steps/<dir path>/<step_id>.json`; the same
/// narration, Action and file-reference rules apply to every caller.
pub(super) fn parse_step_value(
    root: &Path,
    canonical_root: &Path,
    relative: &Path,
    mut step_value: serde_json::Value,
    unit_audio_assets: &HashMap<String, action_narration::ResolvedAudioAsset>,
) -> Result<Step, CourseError> {
    reject_removed_action_vocabulary(&step_value)?;
    let narration_inputs = take_action_narrations(&mut step_value)?;
    let mut step: Step = serde_json::from_value(step_value)?;
    validate_challenge_step(&step)?;
    step.action_narrations = narration_inputs
        .into_iter()
        .map(|narration| -> Result<ActionNarration, CourseError> {
            if narration.during.is_some() && narration.after.is_some() {
                return Err(CourseError::Package(
                    "Action narration `during` and `after` cannot be combined".to_string(),
                ));
            }
            Ok(ActionNarration {
                before: narration
                    .before
                    .map(|phase| load_action_narration_phase(unit_audio_assets, "before", phase))
                    .transpose()?,
                during: narration
                    .during
                    .map(|phase| load_action_narration_phase(unit_audio_assets, "during", phase))
                    .transpose()?,
                after: narration
                    .after
                    .map(|phase| load_action_narration_phase(unit_audio_assets, "after", phase))
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    resolve_action_files(root, canonical_root, &mut step)?;
    validate_step_action_contract(&step)?;
    resolve_wait_caption_durations(&mut step);
    if step.actions.is_empty() {
        return Err(CourseError::Package(format!(
            "step `{}` must contain at least one Action",
            step.step_id
        )));
    }
    if relative.file_stem().and_then(|stem| stem.to_str()) != Some(step.step_id.as_str()) {
        return Err(CourseError::Package(format!(
            "step_file `{}` must match its step_id `{}`",
            relative.display(),
            step.step_id
        )));
    }
    Ok(step)
}

fn validate_challenge_step(step: &Step) -> Result<(), CourseError> {
    let Some(challenge) = step.challenge.as_ref() else {
        return Ok(());
    };
    if challenge.instructions.trim().is_empty() {
        return Err(CourseError::Package(format!(
            "challenge step `{}` instructions must not be empty",
            step.step_id
        )));
    }
    if challenge
        .hint
        .as_deref()
        .is_some_and(|hint| hint.trim().is_empty())
    {
        return Err(CourseError::Package(format!(
            "challenge step `{}` hint must not be empty",
            step.step_id
        )));
    }
    if challenge.checks.len() != 1 {
        return Err(CourseError::Package(format!(
            "challenge step `{}` must contain exactly one COMMAND check",
            step.step_id
        )));
    }
    let check = &challenge.checks[0];
    if check.session_key().trim().is_empty() {
        return Err(CourseError::Package(format!(
            "challenge step `{}` check session_key must not be empty",
            step.step_id
        )));
    }
    if !step.actions.iter().any(|action| {
        matches!(
            action,
            Action::TerminalEnsure(ensure) if ensure.session_key == check.session_key()
        )
    }) {
        return Err(CourseError::Package(format!(
            "challenge step `{}` check session_key `{}` has no matching TerminalEnsure",
            step.step_id,
            check.session_key()
        )));
    }
    if check.command().trim().is_empty() {
        return Err(CourseError::Package(format!(
            "challenge step `{}` check command must not be empty",
            step.step_id
        )));
    }
    if check.accepted_exit_codes().is_empty() {
        return Err(CourseError::Package(format!(
            "challenge step `{}` check accepted_exit_codes must not be empty",
            step.step_id
        )));
    }
    if check
        .expected_output()
        .iter()
        .any(|expected| expected.trim().is_empty())
    {
        return Err(CourseError::Package(format!(
            "challenge step `{}` check expected_output must not contain empty strings",
            step.step_id
        )));
    }
    if check.timeout_ms() == 0 {
        return Err(CourseError::Package(format!(
            "challenge step `{}` check timeout_ms must be greater than zero",
            step.step_id
        )));
    }
    if check.timeout_ms() > MAX_ACTION_TIMEOUT_MS {
        return Err(CourseError::Package(format!(
            "challenge step `{}` check timeout_ms must not exceed {MAX_ACTION_TIMEOUT_MS} ms",
            step.step_id
        )));
    }
    validate_challenge_cwd(&step.step_id, check.cwd())?;
    Ok(())
}

fn validate_challenge_cwd(step_id: &str, cwd: &str) -> Result<(), CourseError> {
    let path = Path::new(cwd);
    if cwd.trim().is_empty()
        || cwd.contains('\\')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CourseError::Package(format!(
            "challenge step `{step_id}` check cwd `{cwd}` must be inside the Unit Workspace"
        )));
    }
    Ok(())
}

/// Reject the pre-rename action vocabulary with a migration hint before serde
/// reports a generic unknown-variant or unknown-field error.
fn reject_removed_action_vocabulary(step_value: &serde_json::Value) -> Result<(), CourseError> {
    let Some(actions) = step_value
        .get("actions")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(());
    };
    let step_id = step_value
        .get("step_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unknown>");
    for (index, action) in actions.iter().enumerate() {
        let Some(action) = action.as_object() else {
            continue;
        };
        let action_index = index + 1;
        let action_type = action.get("type").and_then(serde_json::Value::as_str);
        if let Some(action_type) = action_type {
            let replacement = match action_type {
                "INSERT_CODE" => Some("InsertText"),
                "REPLACE_CODE" => Some("ReplaceText"),
                "DELETE_CODE" => Some("DeleteText"),
                "REVEAL_CODE" => Some("RevealText"),
                _ => None,
            };
            if let Some(replacement) = replacement {
                return Err(CourseError::Package(format!(
                    "step `{step_id}` Action {action_index} uses the removed type `{action_type}`; rename it to `{replacement}`"
                )));
            }
        }
        if matches!(action_type, Some("InsertText" | "ReplaceText")) {
            for (field, replacement) in [("code", "text"), ("code_path", "text_path")] {
                if action.contains_key(field) {
                    return Err(CourseError::Package(format!(
                        "step `{step_id}` Action {action_index} uses the removed field `{field}`; rename it to `{replacement}`"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn resolve_action_files(
    root: &Path,
    canonical_root: &Path,
    step: &mut Step,
) -> Result<(), CourseError> {
    for action in &mut step.actions {
        match action {
            Action::InsertText(action) => {
                let Some(text_path) = action.text_path.clone() else {
                    continue;
                };
                action.text =
                    load_action_text_file(root, canonical_root, "InsertText", &text_path)?;
                if action.text.is_empty() {
                    return Err(CourseError::Package(format!(
                        "InsertText text path `{text_path}` must not resolve to empty text"
                    )));
                }
            }
            Action::ReplaceText(action) => {
                let Some(text_path) = action.text_path.clone() else {
                    continue;
                };
                action.text =
                    load_action_text_file(root, canonical_root, "ReplaceText", &text_path)?;
                if action.text.is_empty() {
                    return Err(CourseError::Package(format!(
                        "ReplaceText text path `{text_path}` must not resolve to empty text"
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_unit_workspace_path(
    step: &Step,
    index: usize,
    field: &str,
    path: &str,
) -> Result<(), CourseError> {
    if path.is_empty() {
        return Err(CourseError::Package(format!(
            "step `{}` Action {} {field} must not be empty",
            step.step_id, index
        )));
    }
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CourseError::Package(format!(
            "step `{}` Action {} {field} `{path}` must stay inside the Unit Workspace",
            step.step_id, index
        )));
    }
    Ok(())
}

fn validate_mutable_workspace_path(
    step: &Step,
    index: usize,
    field: &str,
    path: &str,
) -> Result<(), CourseError> {
    validate_unit_workspace_path(step, index, field, path)?;
    if Path::new(path)
        .components()
        .all(|component| matches!(component, std::path::Component::CurDir))
    {
        return Err(CourseError::Package(format!(
            "step `{}` Action {} {field} `{path}` must not target the Unit Workspace root",
            step.step_id, index
        )));
    }
    Ok(())
}

fn validate_delete_directory_path(
    step: &Step,
    index: usize,
    path: &str,
) -> Result<(), CourseError> {
    validate_mutable_workspace_path(step, index, "DeleteDirectory path", path)
}

fn validate_open_edit_path(
    step: &Step,
    index: usize,
    action_type: &str,
    path: &str,
    opened_path: Option<&str>,
) -> Result<(), CourseError> {
    if path.is_empty() {
        return Err(CourseError::Package(format!(
            "step `{}` Action {} {action_type} path must not be empty",
            step.step_id, index
        )));
    }
    if opened_path != Some(path) {
        return Err(CourseError::Package(format!(
            "step `{}` Action {} {action_type} path `{path}` must be preceded by OpenFile for the same path",
            step.step_id, index
        )));
    }
    Ok(())
}

fn validate_insert_text_target(
    step: &Step,
    index: usize,
    action_type: &str,
    target: &InsertTextTarget,
) -> Result<(), CourseError> {
    validate_anchor(step, index, action_type, "insert", target)
}

fn validate_anchor(
    step: &Step,
    index: usize,
    action_type: &str,
    role: &str,
    anchor: &Anchor,
) -> Result<(), CourseError> {
    let error = |message: &str| {
        CourseError::Package(format!(
            "step `{}` Action {} {action_type} {message}",
            step.step_id, index
        ))
    };
    if anchor.eof && role == "start" {
        return Err(error("start anchor must not be eof"));
    }
    anchor.validate().map_err(|message| error(&message))
}

fn point_column(anchor: &Anchor, end: bool) -> u32 {
    match &anchor.at {
        Some(AnchorAt::Column(column)) => *column,
        _ => match anchor.placement_or_default(end) {
            Placement::Before => 0,
            Placement::After => u32::MAX,
        },
    }
}

fn validate_edit_range(
    step: &Step,
    index: usize,
    action_type: &str,
    range: &EditRange,
) -> Result<(), CourseError> {
    let error = |message: &str| {
        CourseError::Package(format!(
            "step `{}` Action {} {action_type} {message}",
            step.step_id, index
        ))
    };
    if range.expected_text.is_empty() {
        return Err(error("expected_text must not be empty"));
    }
    validate_anchor(step, index, action_type, "start", &range.start)?;
    validate_anchor(step, index, action_type, "end", &range.end)?;

    if let (Some(AnchorAt::Text(start_anchor)), Some(AnchorAt::Text(end_anchor))) =
        (&range.start.at, &range.end.at)
        && range.start.line == range.end.line
        && start_anchor.text == end_anchor.text
        && start_anchor.index.unwrap_or(0) == end_anchor.index.unwrap_or(0)
        && (range.start.placement == Some(Placement::After)
            || range.end.placement == Some(Placement::Before))
    {
        return Err(error("text anchors must form a non-empty range"));
    }

    if !matches!(range.start.at, Some(AnchorAt::Text(_)))
        && !range.start.eof
        && !matches!(range.end.at, Some(AnchorAt::Text(_)))
        && !range.end.eof
    {
        let (Some(start_line), Some(end_line)) = (range.start.line, range.end.line) else {
            return Err(error("anchor must include `line`, `at`, or `eof`"));
        };
        if start_line > end_line
            || (start_line == end_line
                && point_column(&range.start, false) > point_column(&range.end, true))
        {
            return Err(error("point anchors must form a non-empty forward range"));
        }
    }
    Ok(())
}

fn validate_delete_target(
    step: &Step,
    index: usize,
    action_type: &str,
    target: &DeleteTextTarget,
) -> Result<(), CourseError> {
    match target {
        DeleteTextTarget::Range(range) => validate_edit_range(step, index, action_type, range),
        DeleteTextTarget::Cursor(target) if target.cursor.line == 0 || target.cursor.count == 0 => {
            Err(CourseError::Package(format!(
                "step `{}` Action {} {action_type} cursor line and count must be at least 1",
                step.step_id, index
            )))
        }
        DeleteTextTarget::Cursor(target) if target.expected_text.is_empty() => {
            Err(CourseError::Package(format!(
                "step `{}` Action {} {action_type} cursor expected_text must not be empty",
                step.step_id, index
            )))
        }
        DeleteTextTarget::Cursor(_) => Ok(()),
    }
}

fn validate_step_action_contract(step: &Step) -> Result<(), CourseError> {
    let mut opened_path: Option<&str> = None;
    // Caption-only clips (no audio asset) must declare their own positive
    // `duration_ms`, except on `Wait` actions whose `during` caption-only clip
    // omits it and inherits the Wait `duration_ms` during load resolution.
    for (index, action) in step.actions.iter().enumerate() {
        if matches!(action, Action::Wait(_)) {
            continue;
        }
        for phase in [
            step.action_narrations[index].before.as_ref(),
            step.action_narrations[index].during.as_ref(),
            step.action_narrations[index].after.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            for clip in &phase.clips {
                if clip.audio_path.is_none() && clip.duration_ms == 0 {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} caption-only clip requires a positive duration_ms",
                        step.step_id, index
                    )));
                }
            }
        }
    }
    for (index, action) in step.actions.iter().enumerate() {
        match action {
            Action::Wait(action) => {
                let narration = &step.action_narrations[index];
                let effective_wait_ms = action.duration_ms.max(
                    narration
                        .during
                        .as_ref()
                        .and_then(|phase| phase.clips.first())
                        .map_or(0, |clip| clip.duration_ms),
                );
                if effective_wait_ms > MAX_ACTION_TIMEOUT_MS {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} Wait effective duration must not exceed {MAX_ACTION_TIMEOUT_MS} ms",
                        step.step_id, index
                    )));
                }
                if narration.before.is_some() || narration.after.is_some() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} Wait may only carry a `during` clip; `before`/`after` are not allowed",
                        step.step_id, index
                    )));
                }
                if let Some(during) = &narration.during {
                    if during.clips.len() != 1 {
                        return Err(CourseError::Package(format!(
                            "step `{}` Action {} Wait `during` must hold exactly one clip",
                            step.step_id, index
                        )));
                    }
                    let clip = &during.clips[0];
                    // An audio clip declares its own normal range; a caption-only
                    // clip omits `duration_ms` and inherits the effective Wait
                    // duration during load resolution.
                    if clip.audio_path.is_none() && clip.duration_ms != 0 {
                        return Err(CourseError::Package(format!(
                            "step `{}` Action {} Wait `during` caption-only clip must not declare `duration_ms` (it inherits the Wait duration)",
                            step.step_id, index
                        )));
                    }
                }
            }
            Action::OpenFile(action) => {
                if action.path.is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} OpenFile path must not be empty",
                        step.step_id, index
                    )));
                }
                opened_path = Some(&action.path);
            }
            Action::CloseFile(action) => {
                if action.path.trim().is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} CloseFile path must not be empty",
                        step.step_id, index
                    )));
                }
                if opened_path == Some(action.path.as_str()) {
                    // 关闭后继续编辑必须重新 OpenFile，静态校验与 Adapter 的
                    // document_not_open 保持一致。
                    opened_path = None;
                }
            }
            Action::ClosePreview(action) => {
                if action.path.trim().is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} ClosePreview path must not be empty",
                        step.step_id, index
                    )));
                }
            }
            Action::InsertText(action) => {
                if action.path.is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} InsertText path must not be empty",
                        step.step_id, index
                    )));
                }
                validate_unit_workspace_path(step, index, "InsertText path", &action.path)?;
                if opened_path != Some(action.path.as_str()) {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} InsertText path `{}` must be preceded by OpenFile for the same path",
                        step.step_id, index, action.path
                    )));
                }
                validate_insert_text_target(step, index, "InsertText", &action.target)?;
            }
            Action::DeleteText(action) => {
                validate_open_edit_path(step, index, "DeleteText", &action.path, opened_path)?;
                validate_unit_workspace_path(step, index, "DeleteText path", &action.path)?;
                validate_delete_target(step, index, "DeleteText", &action.target)?;
            }
            Action::ReplaceText(action) => {
                validate_open_edit_path(step, index, "ReplaceText", &action.path, opened_path)?;
                validate_unit_workspace_path(step, index, "ReplaceText path", &action.path)?;
                validate_edit_range(step, index, "ReplaceText", &action.target)?;
            }
            Action::CreateDirectory(action) => {
                validate_mutable_workspace_path(step, index, "CreateDirectory path", &action.path)?;
            }
            Action::CreateFile(action) => {
                validate_mutable_workspace_path(step, index, "CreateFile path", &action.path)?;
            }
            Action::DeleteFile(action) => {
                validate_mutable_workspace_path(step, index, "DeleteFile path", &action.path)?;
            }
            Action::DeleteDirectory(action) => {
                validate_delete_directory_path(step, index, &action.path)?;
            }
            Action::TerminalEnsure(action) => {
                if action.session_key.trim().is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} TerminalEnsure session_key must not be empty",
                        step.step_id, index
                    )));
                }
                validate_unit_workspace_path(step, index, "TerminalEnsure cwd", &action.cwd)?;
            }
            Action::TerminalCommand(action) => {
                if action.session_key.trim().is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} TerminalCommand session_key must not be empty",
                        step.step_id, index
                    )));
                }
                if action.command.trim().is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} TerminalCommand command must not be empty",
                        step.step_id, index
                    )));
                }
                if action.timeout_ms == 0 {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} TerminalCommand timeout_ms must be greater than zero",
                        step.step_id, index
                    )));
                }
                if action.timeout_ms > MAX_ACTION_TIMEOUT_MS {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} TerminalCommand timeout_ms must not exceed {MAX_ACTION_TIMEOUT_MS} ms",
                        step.step_id, index
                    )));
                }
                if action.accepted_exit_codes.is_empty() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} TerminalCommand accepted_exit_codes must not be empty",
                        step.step_id, index
                    )));
                }
                if action
                    .expected_output
                    .iter()
                    .any(|expected| expected.trim().is_empty())
                {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} TerminalCommand expected_output must not contain empty strings",
                        step.step_id, index
                    )));
                }
                validate_unit_workspace_path(step, index, "TerminalCommand cwd", &action.cwd)?;
                if let Some(marker) = action.completion_marker.as_deref() {
                    validate_unit_workspace_path(
                        step,
                        index,
                        "TerminalCommand completion_marker",
                        marker,
                    )?;
                }
            }
            Action::JupyterLabOpenNotebook(action) => {
                validate_notebook_path(step, index, "JupyterLabOpenNotebook path", &action.path)?;
            }
            Action::JupyterLabCellSelect(action) => {
                validate_notebook_path(step, index, "JupyterLabCellSelect path", &action.path)?;
                validate_cell_id(step, index, "JupyterLabCellSelect", &action.cell_id)?;
                if !action.scroll && action.align.is_some() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} JupyterLabCellSelect align requires scroll",
                        step.step_id, index
                    )));
                }
            }
            Action::JupyterLabCellRun(action) => {
                validate_notebook_path(step, index, "JupyterLabCellRun path", &action.path)?;
                if let Some(from_cell_id) = action.from_cell_id.as_deref() {
                    validate_cell_id(step, index, "JupyterLabCellRun from_cell_id", from_cell_id)?;
                }
                if let Some(to_cell_id) = action.to_cell_id.as_deref() {
                    validate_cell_id(step, index, "JupyterLabCellRun to_cell_id", to_cell_id)?;
                }
                if action.timeout_ms == 0 {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} JupyterLabCellRun timeout_ms must be greater than zero",
                        step.step_id, index
                    )));
                }
                if action.timeout_ms > MAX_ACTION_TIMEOUT_MS {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} JupyterLabCellRun timeout_ms must not exceed {MAX_ACTION_TIMEOUT_MS} ms",
                        step.step_id, index
                    )));
                }
            }
            Action::JupyterLabRestartKernel(action) => {
                validate_notebook_path(step, index, "JupyterLabRestartKernel path", &action.path)?;
            }
            Action::JupyterLabInterruptKernel(action) => {
                validate_notebook_path(
                    step,
                    index,
                    "JupyterLabInterruptKernel path",
                    &action.path,
                )?;
            }
            Action::JupyterLabOpenPreview(action) => {
                validate_unit_workspace_path(
                    step,
                    index,
                    "JupyterLabOpenPreview path",
                    &action.path,
                )?;
                if !action.path.to_ascii_lowercase().ends_with(".md")
                    && !action.path.to_ascii_lowercase().ends_with(".markdown")
                {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} JupyterLabOpenPreview path must be Markdown",
                        step.step_id, index
                    )));
                }
            }
            Action::JupyterLabCloseDocument(action) => {
                validate_unit_workspace_path(
                    step,
                    index,
                    "JupyterLabCloseDocument path",
                    &action.path,
                )?;
            }
            Action::JupyterLabCellInsertText(action) => {
                validate_notebook_path(step, index, "JupyterLabCellInsertText path", &action.path)?;
                validate_cell_id(step, index, "JupyterLabCellInsertText", &action.cell_id)?;
                validate_insert_text_target(
                    step,
                    index,
                    "JupyterLabCellInsertText",
                    &action.target,
                )?;
            }
            Action::JupyterLabCellDeleteText(action) => {
                validate_notebook_path(step, index, "JupyterLabCellDeleteText path", &action.path)?;
                validate_cell_id(step, index, "JupyterLabCellDeleteText", &action.cell_id)?;
                validate_delete_target(step, index, "JupyterLabCellDeleteText", &action.target)?;
            }
            Action::JupyterLabCellReplaceText(action) => {
                validate_notebook_path(
                    step,
                    index,
                    "JupyterLabCellReplaceText path",
                    &action.path,
                )?;
                validate_cell_id(step, index, "JupyterLabCellReplaceText", &action.cell_id)?;
                validate_edit_range(step, index, "JupyterLabCellReplaceText", &action.target)?;
            }
            Action::JupyterLabCellInsert(action) => {
                validate_notebook_path(step, index, "JupyterLabCellInsert path", &action.path)?;
                validate_cell_id(step, index, "JupyterLabCellInsert", &action.cell_id)?;
                if action.after_cell_id.is_some() && action.before_cell_id.is_some() {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} JupyterLabCellInsert cannot use both after_cell_id and before_cell_id",
                        step.step_id, index
                    )));
                }
                if let Some(after_cell_id) = action.after_cell_id.as_deref() {
                    validate_cell_id(
                        step,
                        index,
                        "JupyterLabCellInsert after_cell_id",
                        after_cell_id,
                    )?;
                }
                if let Some(before_cell_id) = action.before_cell_id.as_deref() {
                    validate_cell_id(
                        step,
                        index,
                        "JupyterLabCellInsert before_cell_id",
                        before_cell_id,
                    )?;
                }
            }
            Action::JupyterLabCellMove(action) => {
                validate_notebook_path(step, index, "JupyterLabCellMove path", &action.path)?;
                validate_cell_id(step, index, "JupyterLabCellMove", &action.cell_id)?;
                if action.count == 0 {
                    return Err(CourseError::Package(format!(
                        "step `{}` Action {} JupyterLabCellMove count must be greater than zero",
                        step.step_id, index
                    )));
                }
            }
            Action::JupyterLabCellDuplicate(action) => {
                validate_notebook_path(step, index, "JupyterLabCellDuplicate path", &action.path)?;
                validate_cell_id(step, index, "JupyterLabCellDuplicate", &action.cell_id)?;
                if let Some(new_cell_id) = action.new_cell_id.as_deref() {
                    validate_cell_id(
                        step,
                        index,
                        "JupyterLabCellDuplicate new_cell_id",
                        new_cell_id,
                    )?;
                }
            }
            Action::JupyterLabCellDelete(action) => {
                validate_notebook_path(step, index, "JupyterLabCellDelete path", &action.path)?;
                validate_cell_id(step, index, "JupyterLabCellDelete", &action.cell_id)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_notebook_path(
    step: &Step,
    index: usize,
    field: &str,
    path: &str,
) -> Result<(), CourseError> {
    if path.trim().is_empty() {
        return Err(CourseError::Package(format!(
            "step `{}` Action {} {field} must not be empty",
            step.step_id, index
        )));
    }
    validate_unit_workspace_path(step, index, field, path)
}

fn validate_cell_id(
    step: &Step,
    index: usize,
    action_type: &str,
    cell_id: &str,
) -> Result<(), CourseError> {
    if cell_id.trim().is_empty() {
        return Err(CourseError::Package(format!(
            "step `{}` Action {} {action_type} cell_id must not be empty",
            step.step_id, index
        )));
    }
    Ok(())
}

/// A `Wait` `during` caption-only clip omits `duration_ms` in the course JSON and
/// resolves to `0` during parsing; backfill it with the owning Wait's
/// `duration_ms` so playback and downstream views receive a concrete value.
/// Call only after `validate_step_action_contract`, which has already confirmed
/// the clip is caption-only with `duration_ms == 0`.
fn resolve_wait_caption_durations(step: &mut Step) {
    for (index, action) in step.actions.iter().enumerate() {
        let Action::Wait(wait) = action else {
            continue;
        };
        let Some(during) = step.action_narrations[index].during.as_mut() else {
            continue;
        };
        let Some(clip) = during.clips.get_mut(0) else {
            continue;
        };
        if clip.audio_path.is_none() {
            clip.duration_ms = wait.duration_ms;
        }
    }
}

fn load_action_text_file(
    root: &Path,
    canonical_root: &Path,
    action_type: &str,
    text_path: &str,
) -> Result<String, CourseError> {
    if text_path.trim().is_empty() {
        return Err(CourseError::Package(format!(
            "{action_type} text path must not be empty"
        )));
    }
    if text_path.contains('\\') {
        return Err(CourseError::Package(format!(
            "{action_type} text path `{text_path}` must use forward slashes"
        )));
    }
    let relative = Path::new(text_path);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CourseError::Package(format!(
            "{action_type} text path `{text_path}` is outside the Course Package"
        )));
    }
    let canonical_path = root.join(relative).canonicalize().map_err(|error| {
        CourseError::Package(format!(
            "{action_type} text path `{text_path}` is unavailable: {error}"
        ))
    })?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(CourseError::Package(format!(
            "{action_type} text path `{text_path}` is outside the Course Package"
        )));
    }
    if !canonical_path.is_file() {
        return Err(CourseError::Package(format!(
            "{action_type} text path `{text_path}` must reference a file"
        )));
    }
    let length = std::fs::metadata(&canonical_path)
        .map_err(|error| {
            CourseError::Package(format!(
                "{action_type} text path `{text_path}` is unavailable: {error}"
            ))
        })?
        .len();
    if length > MAX_TEXT_PATH_BYTES {
        return Err(CourseError::Package(format!(
            "{action_type} text path `{text_path}` is too large ({length} bytes)"
        )));
    }
    std::fs::read_to_string(&canonical_path).map_err(|error| {
        CourseError::Package(format!(
            "{action_type} text path `{text_path}` must be UTF-8 text: {error}"
        ))
    })
}
