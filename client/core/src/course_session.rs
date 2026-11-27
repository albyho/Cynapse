//! Learning session state machine: course opening, progress lifecycle,
//! playback/progress persistence, and command dispatch.
//!
//! Public views are re-exported at this root; import from
//! `crate::course_session::...`.
//!
//! Submodules:
//! - `store` — SQLite-backed state store
//! - `index` — per-open Step indexes shared by playback and progress
//! - `lifecycle` — course open/close and recovery
//! - `playback` — playback position and narration state
//! - `progress` — step/unit completion and progression
//! - `commands` — action command execution and tracking
//! - `units` — unit selection, workspace preparation, and staging loop
//! - `views` — public view types (re-exported)
//! - `tests` — white-box tests (index in tests.rs)

mod commands;
mod index;
mod lifecycle;
mod playback;
mod progress;
mod store;
mod units;
mod views;

use store::{
    clear_resume_cursor, load_progress, load_recent_courses, migrate, persist_selection,
    recover_running_commands, setting, unit_status_key,
};
use views::course_view;
pub use views::{
    ActionNarrationAudioView, ChallengeStatus, ChallengeView, CommandRecoveryView,
    CourseTreeNodeView, CourseView, RecentCourse, StepView, UnitStatus, UnitView,
    find_unit_by_path,
};

use crate::course::{
    ActionNarrationPhase, AdapterId, Course, CourseError, EnvironmentRequirement, LoadedStep,
    PlaybackStep, load_package,
};
use crate::discovery::now_unix_millis;
use crate::server::{PlaybackInterruptionReason, PlaybackResume};
use cynapse_client_i18n::{t, t_with};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CourseSessionState {
    pub active_course: Option<CourseView>,
    pub active_course_path: Option<String>,
    pub selected_unit: Option<UnitView>,
    pub recent_courses: Vec<RecentCourse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_unit_paths: Vec<String>,
    #[serde(default)]
    pub progress_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseSessionOutcome {
    pub state: CourseSessionState,
    pub removed_workspace_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnitResetScope {
    pub workspace_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitLocation {
    pub path: String,
}

impl UnitLocation {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

#[derive(Debug, Clone)]
pub struct UnitTransitionPlan {
    pub location: UnitLocation,
    pub environment_requirements: Vec<EnvironmentRequirement>,
    pub adapters: Vec<AdapterId>,
    pub reset_required: bool,
    pub reset_workspace_paths: Vec<String>,
    pub reset_exclude_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UnitPlaybackPlan {
    pub steps: Vec<PlaybackStep>,
    pub resume: Option<PlaybackResume>,
}

struct UnitResetPlan {
    affected_unit_paths: Vec<String>,
    reset_workspace_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CommandInterruption {
    pub unit_path: String,
    pub step_id: String,
    pub action_index: usize,
    pub reason: PlaybackInterruptionReason,
    pub exit_code: Option<i32>,
    pub output: Option<String>,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandExecutionStart<'a> {
    pub unit_path: &'a str,
    pub step_id: &'a str,
    pub action_index: usize,
    pub command: &'a str,
    pub cwd: &'a str,
    pub timeout_ms: u64,
}

#[derive(Debug)]
pub enum CourseSessionError {
    Sql(rusqlite::Error),
    Io(std::io::Error),
    Course(CourseError),
    UnitNotFound(String),
    NoActiveProgress,
    NoActiveUnit,
    ConfirmationRequired,
    UnitResetNotStarted(String),
    WorkspaceReset(String),
    SchemaTooNew(i64),
}

impl std::fmt::Display for CourseSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Sql(error) => t_with(
                "error.course_state_sqlite",
                &[("error", &error.to_string())],
            ),
            Self::Io(error) => t_with("error.course_state_io", &[("error", &error.to_string())]),
            Self::Course(error) => t_with("error.course_package", &[("error", &error.to_string())]),
            Self::UnitNotFound(unit_path) => {
                t_with("error.unit_not_in_course", &[("unit", unit_path)])
            }
            Self::NoActiveProgress => t("error.no_active_progress"),
            Self::NoActiveUnit => t("error.no_active_unit"),
            Self::ConfirmationRequired => t("error.confirmation_required"),
            Self::UnitResetNotStarted(unit_path) => {
                t_with("error.unit_reset_not_started", &[("unit", unit_path)])
            }
            Self::WorkspaceReset(message) => {
                t_with("error.workspace_reset_failed", &[("error", message)])
            }
            Self::SchemaTooNew(version) => {
                t_with("error.schema_too_new", &[("version", &version.to_string())])
            }
        };
        formatter.write_str(&message)
    }
}

impl std::error::Error for CourseSessionError {}

impl From<rusqlite::Error> for CourseSessionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

impl From<std::io::Error> for CourseSessionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<CourseError> for CourseSessionError {
    fn from(error: CourseError) -> Self {
        Self::Course(error)
    }
}

impl From<serde_json::Error> for CourseSessionError {
    fn from(error: serde_json::Error) -> Self {
        Self::Course(CourseError::Json(error))
    }
}

pub struct CourseSession {
    connection: Connection,
    active_course: Option<Course>,
    active_index: Option<index::CourseStepIndex>,
    active_path: Option<String>,
    active_workspace_path: Option<PathBuf>,
    recent_courses: Vec<RecentCourse>,
    selected_unit_path: Option<String>,
    progress: Option<CourseProgress>,
    changed_units: HashSet<String>,
    progress_revision: u64,
    /// In-memory mirror of a running Command Execution; avoids a database query
    /// on every Action. Startup recovery clears it by marking rows paused.
    running_command: bool,
    #[cfg(test)]
    test_hooks: TestHooks,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LifecycleFailpoint {
    Resources,
    Commit,
}

/// Test-only lifecycle failpoints; production builds compile them out.
#[cfg(test)]
#[derive(Debug, Default)]
struct TestHooks {
    fail_next_open: Option<LifecycleFailpoint>,
    fail_next_restore: Option<LifecycleFailpoint>,
}

#[derive(Debug, Clone)]
pub(super) struct CourseProgress {
    current_unit_path: Option<String>,
    version: String,
    updated_at: u64,
    current_step_id: Option<String>,
    progress: Vec<StepProgressRecord>,
    recovery: Option<CommandRecoveryView>,
    unit_states: Vec<UnitStateRecord>,
}

#[derive(Debug, Clone)]
pub(super) struct StepProgressRecord {
    unit_path: String,
    step_id: String,
    started: bool,
    completed: bool,
    challenge_status: ChallengeStatus,
    challenge_attempts: u32,
    challenge_checked_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub(super) struct UnitStateRecord {
    unit_path: String,
    status: UnitStatus,
    current_step_id: Option<String>,
    updated_at: u64,
}

struct ProgressIndex<'a> {
    steps: HashMap<&'a str, HashMap<&'a str, &'a StepProgressRecord>>,
}

impl<'a> ProgressIndex<'a> {
    fn new(progress: &'a CourseProgress) -> Self {
        let mut steps: HashMap<&'a str, HashMap<&'a str, &'a StepProgressRecord>> = HashMap::new();
        for step in &progress.progress {
            steps
                .entry(step.unit_path.as_str())
                .or_default()
                .insert(step.step_id.as_str(), step);
        }
        Self { steps }
    }

    fn step(&self, unit_path: &str, step_id: &str) -> Option<&'a StepProgressRecord> {
        self.steps.get(unit_path)?.get(step_id).copied()
    }

    fn skipped_challenges(&self, unit_path: &str) -> u32 {
        self.steps
            .get(unit_path)
            .map(|steps| {
                steps
                    .values()
                    .filter(|step| step.challenge_status == ChallengeStatus::Skipped)
                    .count() as u32
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
fn find_unit(nodes: &[CourseTreeNodeView], path: &str) -> Option<UnitView> {
    find_unit_by_path(nodes, path).cloned()
}

/// Whether every Step of one Unit is completed. Takes the Steps as an iterator
/// so a caller can hand over its own per-Unit slice or a filtered scan of the
/// whole course, whichever it already has.
fn unit_is_completed<'a>(
    steps: impl IntoIterator<Item = &'a LoadedStep>,
    progress: Option<&ProgressIndex<'_>>,
    unit_path: &str,
) -> bool {
    let mut found = false;
    for loaded in steps {
        found = true;
        if !progress
            .and_then(|progress| progress.step(unit_path, &loaded.step.step_id))
            .is_some_and(|step| step.completed)
        {
            return false;
        }
    }
    found
}

fn unit_has_recorded_progress(progress: Option<&CourseProgress>, unit_path: &str) -> bool {
    progress.is_some_and(|progress| {
        progress
            .progress
            .iter()
            .any(|step| step.unit_path == unit_path && (step.started || step.completed))
            || progress
                .unit_states
                .iter()
                .any(|state| state.unit_path == unit_path && state.status != UnitStatus::NotStarted)
    })
}

/// Reset deletes the Unit's `unit_states` row and leaves one zeroed
/// `step_states` row per Step. Progress creation keeps a `not_started`
/// `unit_states` row, so its zeroed rows still mean "top up, do not replace".
fn unit_was_reset(progress: &CourseProgress, unit_path: &str) -> bool {
    if progress
        .unit_states
        .iter()
        .any(|state| state.unit_path == unit_path)
    {
        return false;
    }
    let mut steps = progress
        .progress
        .iter()
        .filter(|step| step.unit_path == unit_path);
    let Some(first) = steps.next() else {
        return false;
    };
    !first.started && !first.completed && steps.all(|step| !step.started && !step.completed)
}

struct UnitProgressReset<'a> {
    course_id: &'a str,
    clear_current_position: bool,
    affected_unit_paths: &'a [String],
    current_steps: &'a [(String, String)],
    now: i64,
}

fn reset_unit_progress(
    connection: &Connection,
    reset: UnitProgressReset<'_>,
) -> Result<(), CourseSessionError> {
    let transaction = connection.unchecked_transaction()?;
    for unit_path in reset.affected_unit_paths {
        transaction.execute(
            "
            DELETE FROM command_executions
            WHERE course_id = ?1 AND unit_path = ?2
            ",
            rusqlite::params![reset.course_id, unit_path],
        )?;
        transaction.execute(
            "
            DELETE FROM step_states
            WHERE course_id = ?1 AND unit_path = ?2
            ",
            rusqlite::params![reset.course_id, unit_path],
        )?;
        transaction.execute(
            "
            DELETE FROM unit_states
            WHERE course_id = ?1 AND unit_path = ?2
            ",
            rusqlite::params![reset.course_id, unit_path],
        )?;
    }
    transaction.execute(
        "
        UPDATE course_progress
        SET current_unit_path = CASE WHEN ?1 THEN NULL ELSE current_unit_path END,
            current_step_id = CASE WHEN ?1 THEN NULL ELSE current_step_id END,
            updated_at = ?2
        WHERE id = ?3
        ",
        rusqlite::params![reset.clear_current_position, reset.now, reset.course_id],
    )?;
    let mut insert_step = transaction.prepare(
        "
        INSERT OR IGNORE INTO step_states(
                course_id, unit_path, step_id, started, completed, updated_at
        )
            VALUES (?1, ?2, ?3, 0, 0, ?4)
        ",
    )?;
    for (unit_path, step_id) in reset.current_steps {
        insert_step.execute(rusqlite::params![
            reset.course_id,
            unit_path,
            step_id,
            reset.now
        ])?;
    }
    drop(insert_step);
    transaction.commit()?;
    Ok(())
}

impl CourseSession {
    pub fn new(db_path: impl Into<PathBuf>) -> Result<Self, CourseSessionError> {
        let mut session = Self::create_unrestored(db_path)?;
        session.restore()?;
        Ok(session)
    }

    fn create_unrestored(db_path: impl Into<PathBuf>) -> Result<Self, CourseSessionError> {
        let db_path = db_path.into();
        if let Some(parent) = db_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&db_path)?;
        migrate(&connection)?;
        recover_running_commands(&connection, now_unix_millis() as i64)?;

        Ok(Self {
            connection,
            active_course: None,
            active_index: None,
            active_path: None,
            active_workspace_path: None,
            recent_courses: Vec::new(),
            selected_unit_path: None,
            progress: None,
            changed_units: HashSet::new(),
            progress_revision: 0,
            running_command: false,
            #[cfg(test)]
            test_hooks: TestHooks::default(),
        })
    }

    pub(super) fn mark_unit_changed(&mut self, unit_path: &str) {
        self.changed_units.insert(unit_path.to_string());
        self.progress_revision = self.progress_revision.wrapping_add(1);
    }

    pub fn state(&self) -> CourseSessionState {
        let workspace_path = self
            .active_workspace()
            .map(|path| path.to_string_lossy().to_string());
        let active_course = self
            .active_course
            .as_ref()
            .map(|course| course_view(course, self.progress.as_ref(), workspace_path.as_deref()));
        let selected_unit = active_course.as_ref().and_then(|course| {
            let unit_path = self.selected_unit_path.as_deref()?;
            find_unit_by_path(&course.tree, unit_path).cloned()
        });
        let mut changed_unit_paths = self.changed_units.iter().cloned().collect::<Vec<_>>();
        changed_unit_paths.sort();
        CourseSessionState {
            active_course,
            active_course_path: self.active_path.clone(),
            selected_unit,
            recent_courses: self.recent_courses.clone(),
            changed_unit_paths,
            progress_revision: self.progress_revision,
        }
    }

    pub fn clear_active_course_in_memory(&mut self) -> CourseSessionState {
        self.clear_active_course();
        self.active_path = None;
        self.active_workspace_path = None;
        self.selected_unit_path = None;
        self.progress = None;
        self.running_command = false;
        self.state()
    }

    pub fn active_course(&self) -> Option<&Course> {
        self.active_course.as_ref()
    }

    fn active_step_index(&self) -> Option<&index::CourseStepIndex> {
        self.active_index.as_ref()
    }

    pub(super) fn set_active_course(&mut self, course: Course) {
        self.active_index = Some(index::CourseStepIndex::new(&course));
        self.active_course = Some(course);
    }

    pub(super) fn clear_active_course(&mut self) {
        self.active_course = None;
        self.active_index = None;
    }

    pub fn terminal_command_at(
        &self,
        unit_path: &str,
        step_id: &str,
        action_index: usize,
    ) -> Option<&crate::course::TerminalCommandAction> {
        let course = self.active_course.as_ref()?;
        let step = self
            .active_index
            .as_ref()?
            .step(course, unit_path, step_id)?;
        match step.step.actions.get(action_index)? {
            crate::course::Action::TerminalCommand(command) => Some(command),
            _ => None,
        }
    }

    pub fn active_workspace(&self) -> Option<PathBuf> {
        self.active_workspace_path
            .as_ref()
            .filter(|path| path.is_dir())
            .cloned()
    }

    fn ensure_active_resources(&self) -> Result<(), CourseSessionError> {
        let Some(package_path) = self.active_path.as_deref() else {
            return Ok(());
        };
        let Some(workspace) = self.active_workspace_path.as_deref() else {
            return Ok(());
        };
        ensure_course_resources(Path::new(package_path), workspace)
    }

    pub fn current_unit_workspace_relative_path(&self) -> Option<String> {
        self.active_workspace()?;
        self.progress.as_ref()?.current_unit_path.clone()
    }

    pub fn current_unit_path(&self) -> Option<&str> {
        self.progress.as_ref()?.current_unit_path.as_deref()
    }

    pub fn required_adapters_for_unit(&self, unit_path: &str) -> Vec<AdapterId> {
        self.active_course
            .as_ref()
            .map(|course| course.effective_adapters_for_path(unit_path))
            .unwrap_or_default()
    }

    /// The external Adapter identity the Protocol server reports for the
    /// current course. Validation allows at most one Adapter per node, so the
    /// first effective requirement is the whole requirement.
    pub fn selected_adapter(&self) -> Option<AdapterId> {
        match self.selected_unit_path.as_deref() {
            Some(unit_path) => self.required_adapters_for_unit(unit_path).first().copied(),
            None => self
                .active_course
                .as_ref()
                .and_then(|course| course.tree.adapters.first().copied()),
        }
    }

    pub fn environment_requirements_for_unit(
        &self,
        unit_path: &str,
    ) -> Vec<EnvironmentRequirement> {
        self.active_course
            .as_ref()
            .map(|course| course.effective_environment_requirements_for_path(unit_path))
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) fn fail_next_open(&mut self, failpoint: LifecycleFailpoint) {
        self.test_hooks.fail_next_open = Some(failpoint);
    }

    #[cfg(test)]
    pub(super) fn fail_next_restore(&mut self, failpoint: LifecycleFailpoint) {
        self.test_hooks.fail_next_restore = Some(failpoint);
    }

    /// Cheap signature of the visible Course Progress. The playback forwarder
    /// uses it to avoid rebuilding the full Course View when a Playing or
    /// Narrating event did not change any course-visible progress.
    pub fn state_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let Some(progress) = self.progress.as_ref() else {
            return 0;
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        progress.current_unit_path.hash(&mut hasher);
        progress.current_step_id.hash(&mut hasher);
        if let Some(recovery) = &progress.recovery {
            recovery.unit_path.hash(&mut hasher);
            recovery.step_id.hash(&mut hasher);
            recovery.action_index.hash(&mut hasher);
            recovery.command.hash(&mut hasher);
            recovery.reason.as_str().hash(&mut hasher);
            recovery.output.hash(&mut hasher);
            recovery.exit_code.hash(&mut hasher);
            recovery.output_truncated.hash(&mut hasher);
        }
        for state in &progress.unit_states {
            state.unit_path.hash(&mut hasher);
            unit_status_key(state.status).hash(&mut hasher);
            state.current_step_id.hash(&mut hasher);
        }
        for step in &progress.progress {
            step.unit_path.hash(&mut hasher);
            step.step_id.hash(&mut hasher);
            step.started.hash(&mut hasher);
            step.completed.hash(&mut hasher);
            store::challenge_status_key(step.challenge_status).hash(&mut hasher);
            step.challenge_attempts.hash(&mut hasher);
        }
        hasher.finish()
    }
}

// The adapter truncates terminal output to the same budget before it sends an
// ACK; both stay far below the WebSocket message limit.
pub(crate) const MAX_COMMAND_OUTPUT_BYTES: usize = 128 * 1024;
const OUTPUT_TRUNCATION_MARKER: &str = "[Cynapse: output truncated]\n";

fn bound_command_output(raw: String, already_truncated: bool) -> (Option<String>, bool) {
    if raw.is_empty() {
        return (None, already_truncated);
    }
    if raw.len() <= MAX_COMMAND_OUTPUT_BYTES {
        return (Some(raw), already_truncated);
    }
    let marker_bytes = OUTPUT_TRUNCATION_MARKER.len();
    let tail_bytes = MAX_COMMAND_OUTPUT_BYTES - marker_bytes;
    let mut start = raw.len() - tail_bytes;
    while !raw.is_char_boundary(start) {
        start += 1;
    }
    (
        Some(format!("{OUTPUT_TRUNCATION_MARKER}{}", &raw[start..])),
        true,
    )
}

fn nearest_existing_path(path: &Path) -> Option<PathBuf> {
    let mut candidate = Some(path);
    while let Some(current) = candidate {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        candidate = current.parent();
    }
    None
}

fn is_retryable_file_lock(_error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(_error.raw_os_error(), Some(5 | 32 | 33))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn is_workspace_path_lock(error: &CourseSessionError) -> bool {
    match error {
        CourseSessionError::Io(io) => is_retryable_file_lock(io),
        _ => false,
    }
}

fn clear_course_workspace(workspace: &Path) -> Result<(), CourseSessionError> {
    validate_workspace_path(workspace, workspace)?;
    std::fs::create_dir_all(workspace)?;
    for entry in std::fs::read_dir(workspace)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".gitkeep" || name == crate::runtime::COURSE_ENVIRONMENT_DIR {
            continue;
        }
        remove_workspace_entry(&entry.path())?;
    }
    Ok(())
}

fn remove_workspace_entry(path: &Path) -> Result<(), CourseSessionError> {
    if std::fs::symlink_metadata(path)?.file_type().is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn ensure_course_workspace(course: &Course) -> Result<(), CourseSessionError> {
    let workspace = course.workspace_path();
    reject_workspace_symlink(&workspace)?;
    let canonical_root = course.package_root.canonicalize()?;
    let existing_parent = workspace
        .parent()
        .and_then(nearest_existing_path)
        .ok_or_else(|| {
            CourseSessionError::Course(CourseError::Package(format!(
                "course workspace `{}` has no existing parent",
                workspace.display()
            )))
        })?;
    if !existing_parent.canonicalize()?.starts_with(&canonical_root) {
        return Err(CourseSessionError::Course(CourseError::Package(
            "course workspace is outside the Course Package".to_string(),
        )));
    }
    std::fs::create_dir_all(&workspace)?;
    reject_workspace_symlink(&workspace)?;
    let canonical_workspace = workspace.canonicalize()?;
    if !canonical_workspace.starts_with(&canonical_root) || !canonical_workspace.is_dir() {
        return Err(CourseSessionError::Course(CourseError::Package(
            "course workspace must be a directory inside the Course Package".to_string(),
        )));
    }
    Ok(())
}

fn normalize_package_path(path: &Path) -> Result<PathBuf, CourseSessionError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    // A deleted package cannot be canonicalized as a whole, but its parent can
    // still expand Windows 8.3 short names (RUNNER~1 and friends), so join the
    // missing tail onto the resolved ancestor instead of keeping the raw path.
    let normalized = std::fs::canonicalize(&absolute)
        .ok()
        .or_else(|| canonicalize_lenient(&absolute))
        .unwrap_or(absolute);
    #[cfg(windows)]
    {
        let text = normalized.to_string_lossy();
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(stripped));
        }
    }
    Ok(normalized)
}

fn canonicalize_lenient(absolute: &Path) -> Option<PathBuf> {
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut current = absolute;
    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(current) {
            for segment in missing.iter().rev() {
                resolved.push(segment);
            }
            return Some(resolved);
        }
        missing.push(current.file_name()?.to_os_string());
        current = current.parent()?;
    }
}

/// Overwrite copy of a Seed tree into an empty or replaceable Workspace.
fn copy_directory_overlay(
    source: &Path,
    destination: &Path,
    exclusions: &HashSet<PathBuf>,
) -> Result<(), CourseSessionError> {
    copy_seed_tree(source, destination, exclusions, false)
}

/// 复制一个 Seed 目录。`exclusions` 里的目录（后代节点的 Seed 根）整棵跳过，
/// 保证 `seeds/<unit path>/` 中只有归属于本 Unit 的内容进入 Workspace。
/// `skip_existing` 为真时只补缺失文件，不覆盖学员已有内容。
fn copy_seed_tree(
    source: &Path,
    destination: &Path,
    exclusions: &HashSet<PathBuf>,
    skip_existing: bool,
) -> Result<(), CourseSessionError> {
    reject_workspace_symlink(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(CourseSessionError::WorkspaceReset(format!(
                "course content contains unsupported symbolic link: {}",
                entry.path().display()
            )));
        }
        if exclusions.contains(&entry.path()) {
            continue;
        }
        let target = destination.join(entry.file_name());
        reject_workspace_symlink(&target)?;
        if file_type.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_seed_tree(&entry.path(), &target, exclusions, skip_existing)?;
        } else if file_type.is_file() && !(skip_existing && target.exists()) {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

const RESOURCE_TEMP_DIR: &str = ".cynapse-resources-tmp";

fn prepare_course_resources(
    package_path: &Path,
    workspace: &Path,
) -> Result<(), CourseSessionError> {
    replace_course_resources(package_path, workspace, |source, target| {
        copy_directory_overlay(source, target, &HashSet::new())
    })
}

fn ensure_course_resources(
    package_path: &Path,
    workspace: &Path,
) -> Result<(), CourseSessionError> {
    let target = workspace.join("resources");
    validate_workspace_path(workspace, &target)?;
    remove_path_if_exists(&workspace.join(RESOURCE_TEMP_DIR))?;
    if is_real_directory(&target) {
        return Ok(());
    }
    replace_course_resources(package_path, workspace, |source, target| {
        copy_directory_overlay(source, target, &HashSet::new())
    })
}

fn replace_course_resources<F>(
    package_path: &Path,
    workspace: &Path,
    copy: F,
) -> Result<(), CourseSessionError>
where
    F: FnOnce(&Path, &Path) -> Result<(), CourseSessionError>,
{
    let source = package_path.join("resources");
    let target = workspace.join("resources");
    let temporary = workspace.join(RESOURCE_TEMP_DIR);
    validate_workspace_path(workspace, &target)?;
    std::fs::create_dir_all(workspace)?;
    remove_path_if_exists(&temporary)?;
    if !source.is_dir() {
        return remove_path_if_exists(&target);
    }
    std::fs::create_dir(&temporary)?;
    if let Err(error) = copy(&source, &temporary) {
        remove_temp_quietly(&temporary);
        return Err(error);
    }
    if let Err(error) = remove_path_if_exists(&target) {
        remove_temp_quietly(&temporary);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temporary, &target) {
        remove_temp_quietly(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn remove_temp_quietly(path: &Path) {
    let _ = remove_path_if_exists(path);
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}

fn remove_path_if_exists(path: &Path) -> Result<(), CourseSessionError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => remove_workspace_entry(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 只补 Seed 里有、目标里没有的文件；已有文件一律不动。
/// 每个 Unit 只有一层 Seed，因此不存在层间覆盖；`exclusions` 用于跳过后代 Unit
/// 的 Seed 目录，避免把它们的起始文件补给本 Unit。
fn top_up_missing_seed_files(
    seeds: &[PathBuf],
    destination: &Path,
    exclusions: &HashSet<PathBuf>,
) -> Result<(), CourseSessionError> {
    for seed in seeds {
        top_up_missing_from(seed, destination, exclusions)?;
    }
    Ok(())
}

fn top_up_missing_from(
    source: &Path,
    destination: &Path,
    exclusions: &HashSet<PathBuf>,
) -> Result<(), CourseSessionError> {
    copy_seed_tree(source, destination, exclusions, true)
}

fn remove_directory_if_empty(path: &Path) -> Result<(), CourseSessionError> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                std::fs::remove_dir(path)?;
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Rejects any symlink or junction on the path from the Course Workspace root
/// to `target`. A malformed or malicious Course Package must never make a
/// workspace operation follow an ancestor link outside the managed tree.
pub fn validate_workspace_path(
    workspace_root: &Path,
    target: &Path,
) -> Result<(), CourseSessionError> {
    let relative = target.strip_prefix(workspace_root).map_err(|_| {
        CourseSessionError::WorkspaceReset(format!(
            "workspace path {} is outside {}",
            target.display(),
            workspace_root.display()
        ))
    })?;
    reject_workspace_symlink(workspace_root)?;
    let mut current = workspace_root.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => {
                current.push(part);
                reject_workspace_symlink(&current)?;
            }
            _ => {
                return Err(CourseSessionError::WorkspaceReset(format!(
                    "workspace path {} must stay inside {}",
                    target.display(),
                    workspace_root.display()
                )));
            }
        }
    }
    Ok(())
}

fn reject_workspace_symlink(path: &Path) -> Result<(), CourseSessionError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CourseSessionError::WorkspaceReset(format!(
                "workspace path {} is a symbolic link or junction",
                path.display()
            )))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 把 `target` 清回「本 Unit 自有」的初始状态：删除本 Unit 自己的文件与目录，
/// 但保留 `descendants` 中列出的后代 Unit 工作区（及其路径容器目录），就地清理、
/// 不搬动后代。用于重置/清空父 Unit 工作区时，避免把已学过的子 Unit 磁盘工作区
/// 一并清掉。`descendants` 为后代 Unit 的绝对工作区路径；某目录只要等于其中某个
/// 路径、或作为它的祖先路径容器，就排除在外。
fn clear_own_workspace_entries(
    target: &Path,
    descendants: &[PathBuf],
) -> Result<(), CourseSessionError> {
    reject_workspace_symlink(target)?;
    if !target.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(target)? {
        let entry = entry?;
        let entry_path = entry.path();
        let is_descendant = descendants
            .iter()
            .any(|descendant| descendant.strip_prefix(&entry_path).is_ok());
        if is_descendant {
            continue;
        }
        remove_workspace_entry(&entry_path)?;
    }
    Ok(())
}

/// Resolve one Action path. Read paths may use `..` inside the Course Workspace;
/// write targets and terminal locations must stay inside the Unit Workspace.
/// See `spec.md` §4.1.
fn prepare_unit_action(
    package: &Course,
    action: &crate::course::Action,
    unit_path: &str,
    resource_root: &Path,
) -> Result<crate::course::Action, CourseSessionError> {
    use crate::course::Action;

    let mut prepared = action.clone();
    match &mut prepared {
        Action::Present | Action::Wait(_) => {}
        Action::OpenFile(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::ReadOnly)?;
        }
        Action::OpenPreview(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::ReadOnly)?;
        }
        Action::OpenFileWithPreview(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::ReadOnly)?;
        }
        Action::CloseFile(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::ReadOnly)?;
        }
        Action::ClosePreview(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::ReadOnly)?;
        }
        Action::RevealText(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::ReadOnly)?;
        }
        Action::ClearReveal => {}
        Action::SetCursor(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::ReadOnly)?;
        }
        Action::InsertText(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::DeleteText(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::ReplaceText(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::CreateDirectory(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::CreateFile(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::DeleteFile(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::DeleteDirectory(action) => {
            let resolved =
                unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
            if resolved == unit_path {
                return Err(CourseSessionError::Course(CourseError::Package(format!(
                    "DeleteDirectory path `{}` must not target the Unit Workspace root",
                    action.path
                ))));
            }
            action.path = resolved;
        }
        Action::TerminalEnsure(action) => {
            action.session_key = unit_session_key(unit_path, &action.session_key);
            action.cwd = unit_action_path(unit_path, &action.cwd, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::TerminalCommand(action) => {
            action.session_key = unit_session_key(unit_path, &action.session_key);
            action.cwd = unit_action_path(unit_path, &action.cwd, PathPolicy::UnitWorkspaceOnly)?;
            if let Some(marker) = action.completion_marker.as_deref() {
                action.completion_marker = Some(unit_action_path(
                    unit_path,
                    marker,
                    PathPolicy::UnitWorkspaceOnly,
                )?);
            }
            action.command = package.resolve_resource_aliases_for_path_with_root(
                &action.command,
                unit_path,
                resource_root,
            )?;
        }
        Action::JupyterLabOpenNotebook(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellSelect(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellRun(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabRestartKernel(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabInterruptKernel(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabOpenPreview(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCloseDocument(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellInsertText(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellDeleteText(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellReplaceText(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellInsert(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellMove(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellDuplicate(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
        Action::JupyterLabCellDelete(action) => {
            action.path = unit_action_path(unit_path, &action.path, PathPolicy::UnitWorkspaceOnly)?;
        }
    }
    Ok(prepared)
}

pub fn unit_session_key(unit_path: &str, session_key: &str) -> String {
    format!("{unit_path}::{session_key}")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathPolicy {
    /// Read Actions may use `..` as long as they stay inside the Course Workspace.
    ReadOnly,
    /// Write targets and terminal locations must stay inside the Unit Workspace.
    UnitWorkspaceOnly,
}

fn unit_action_path(
    unit_path: &str,
    action_path: &str,
    policy: PathPolicy,
) -> Result<String, CourseSessionError> {
    if action_path.contains("{{resource:") {
        return Err(CourseSessionError::Course(CourseError::Package(
            "action path must not use resource aliases".to_string(),
        )));
    }
    let relative = Path::new(action_path);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CourseSessionError::Course(CourseError::Package(format!(
            "action path `{action_path}` escapes the Course Workspace"
        ))));
    }
    let mut parts = unit_path.split('/').map(str::to_string).collect::<Vec<_>>();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(value) => {
                parts.push(value.to_string_lossy().to_string());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if policy == PathPolicy::UnitWorkspaceOnly {
                    return Err(CourseSessionError::Course(CourseError::Package(format!(
                        "path `{action_path}` must stay inside the Unit Workspace"
                    ))));
                }
                if parts.pop().is_none() {
                    return Err(CourseSessionError::Course(CourseError::Package(format!(
                        "action path `{action_path}` escapes the Course Workspace"
                    ))));
                }
            }
            _ => {
                return Err(CourseSessionError::Course(CourseError::Package(format!(
                    "action path `{action_path}` escapes the Course Workspace"
                ))));
            }
        }
    }
    if parts.is_empty() {
        return Err(CourseSessionError::Course(CourseError::Package(format!(
            "action path `{action_path}` resolves to the Course Workspace root"
        ))));
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests;
