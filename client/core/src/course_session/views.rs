use super::CourseProgress;
use crate::course::{AdapterId, Course, CourseNode, LoadedStep};
use crate::server::PlaybackInterruptionReason;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::{ProgressIndex, unit_is_completed};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CourseView {
    pub id: String,
    pub title: String,
    pub version: String,
    pub adapters: Vec<AdapterId>,
    pub workspace_path: Option<String>,
    pub tree: Vec<CourseTreeNodeView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_unit_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<CommandRecoveryView>,
}

impl CourseView {
    /// Whether any Unit has started or completed. Selection and the resume
    /// cursor are not progress.
    pub fn has_progress(&self) -> bool {
        fn node_has_progress(node: &CourseTreeNodeView) -> bool {
            node.unit
                .as_ref()
                .is_some_and(|unit| unit.status != UnitStatus::NotStarted)
                || node.children.iter().any(node_has_progress)
        }
        self.tree.iter().any(node_has_progress)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CourseTreeNodeView {
    pub path: String,
    pub title: String,
    pub completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<UnitView>,
    pub children: Vec<CourseTreeNodeView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnitView {
    pub path: String,
    pub title: String,
    pub steps: Vec<StepView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step_id: Option<String>,
    pub adapters: Vec<AdapterId>,
    pub status: UnitStatus,
    pub completed: bool,
    pub skipped_challenges: u32,
}

/// Finds the Unit with the given Node Path in a Course Tree.
pub fn find_unit_by_path<'a>(nodes: &'a [CourseTreeNodeView], path: &str) -> Option<&'a UnitView> {
    for node in nodes {
        if let Some(unit) = node.unit.as_ref().filter(|unit| unit.path == path) {
            return Some(unit);
        }
        if let Some(unit) = find_unit_by_path(&node.children, path) {
            return Some(unit);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitStatus {
    NotStarted,
    InProgress,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandRecoveryView {
    pub unit_path: String,
    pub step_id: String,
    pub action_index: usize,
    pub command: String,
    pub reason: PlaybackInterruptionReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub output_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepView {
    pub step_id: String,
    pub title: String,
    pub challenge: Option<ChallengeView>,
    pub challenge_status: ChallengeStatus,
    pub challenge_attempts: u32,
    pub started: bool,
    pub completed: bool,
    #[serde(default)]
    pub requires_human: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChallengeView {
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub allow_skip: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeStatus {
    NotStarted,
    Failed,
    Passed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionNarrationAudioView {
    pub audio_path: Option<String>,
    pub offset_ms: Option<u64>,
    pub duration_ms: u64,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentCourse {
    pub path: String,
    pub id: String,
    pub title: String,
}

/// Builds the tree embedded in every `CourseSession::state()` snapshot.
///
/// Linear: content, Unit, Step, and Unit-state indexes are built once here and
/// shared by every node. No per-Unit scans of the full Step list.
pub(super) fn course_view(
    course: &Course,
    progress: Option<&CourseProgress>,
    workspace_path: Option<&str>,
) -> CourseView {
    let progress_index = progress.map(ProgressIndex::new);
    let unit_index = course
        .units
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<HashMap<_, _>>();
    let step_index = course.steps.iter().fold(
        HashMap::<&str, Vec<&LoadedStep>>::new(),
        |mut index, step| {
            index.entry(step.unit_path.as_str()).or_default().push(step);
            index
        },
    );
    let unit_state_index = progress
        .map(|value| {
            value
                .unit_states
                .iter()
                .map(|state| (state.unit_path.as_str(), state))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let units_with_progress = progress
        .map(|value| {
            let mut units = HashSet::new();
            for step in &value.progress {
                if step.started || step.completed {
                    units.insert(step.unit_path.as_str());
                }
            }
            for state in &value.unit_states {
                if state.status != UnitStatus::NotStarted {
                    units.insert(state.unit_path.as_str());
                }
            }
            units
        })
        .unwrap_or_default();
    CourseView {
        id: course.manifest.id.clone(),
        title: course.manifest.title.clone(),
        version: course.manifest.version.clone(),
        adapters: course.tree.adapters.clone(),
        workspace_path: workspace_path.map(str::to_string),
        tree: course
            .tree
            .children
            .iter()
            .map(|node| {
                course_tree_node(
                    node,
                    "",
                    &unit_index,
                    &step_index,
                    &unit_state_index,
                    &units_with_progress,
                    progress,
                    progress_index.as_ref(),
                )
            })
            .collect(),
        current_unit_path: progress.and_then(|progress| progress.current_unit_path.clone()),
        current_step_id: progress.and_then(|progress| progress.current_step_id.clone()),
        recovery: progress.and_then(|progress| progress.recovery.clone()),
    }
}

#[allow(clippy::too_many_arguments)]
fn course_tree_node(
    node: &CourseNode,
    parent_path: &str,
    unit_index: &HashMap<&str, &crate::course::UnitEntry>,
    step_index: &HashMap<&str, Vec<&LoadedStep>>,
    unit_state_index: &HashMap<&str, &super::UnitStateRecord>,
    units_with_progress: &HashSet<&str>,
    progress: Option<&CourseProgress>,
    progress_index: Option<&ProgressIndex<'_>>,
) -> CourseTreeNodeView {
    let path = if parent_path.is_empty() {
        node.id.clone()
    } else {
        format!("{parent_path}/{}", node.id)
    };
    let children = node
        .children
        .iter()
        .map(|child| {
            course_tree_node(
                child,
                &path,
                unit_index,
                step_index,
                unit_state_index,
                units_with_progress,
                progress,
                progress_index,
            )
        })
        .collect::<Vec<_>>();
    let unit = if node.has_steps() {
        let entry = unit_index
            .get(path.as_str())
            .copied()
            .expect("normalized unit entry must exist");
        let steps = step_index
            .get(entry.path.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        Some(unit_view(
            entry,
            steps,
            progress,
            progress_index,
            unit_state_index.get(entry.path.as_str()).copied(),
            units_with_progress.contains(entry.path.as_str()),
        ))
    } else {
        None
    };
    // A node is complete when its own steps are done and every child is
    // complete. A node without steps contributes nothing to that judgement,
    // so it is vacuously done by itself.
    let own_done = unit.as_ref().is_none_or(|unit| unit.completed);
    let completed = own_done && children.iter().all(|child| child.completed);
    CourseTreeNodeView {
        path,
        title: node.title.clone(),
        completed,
        unit,
        children,
    }
}

fn unit_view(
    entry: &crate::course::UnitEntry,
    steps: &[&LoadedStep],
    progress: Option<&CourseProgress>,
    progress_index: Option<&ProgressIndex<'_>>,
    unit_state: Option<&super::UnitStateRecord>,
    has_progress: bool,
) -> UnitView {
    let completed = unit_is_completed(steps.iter().copied(), progress_index, &entry.path);
    let course_current =
        progress.and_then(|value| value.current_unit_path.as_deref()) == Some(entry.path.as_str());
    let stored_status = unit_state
        .map(|state| state.status)
        .unwrap_or(UnitStatus::NotStarted);
    let current_step_id = unit_state
        .and_then(|state| state.current_step_id.clone())
        .or_else(|| {
            course_current
                .then(|| progress.and_then(|value| value.current_step_id.clone()))
                .flatten()
        });
    let status = if completed {
        UnitStatus::Completed
    } else if stored_status == UnitStatus::Paused {
        UnitStatus::Paused
    } else if has_progress {
        UnitStatus::InProgress
    } else {
        UnitStatus::NotStarted
    };
    UnitView {
        path: entry.path.clone(),
        title: entry.unit.title.clone(),
        adapters: entry.unit.adapters.clone(),
        steps: steps
            .iter()
            .map(|loaded| {
                let progress_entry =
                    progress_index.and_then(|index| index.step(&entry.path, &loaded.step.step_id));
                StepView {
                    step_id: loaded.step.step_id.clone(),
                    title: loaded.step.title.clone(),
                    challenge: loaded
                        .step
                        .challenge
                        .as_ref()
                        .map(|challenge| ChallengeView {
                            instructions: challenge.instructions.clone(),
                            hint: challenge.hint.clone(),
                            allow_skip: challenge.allow_skip,
                        }),
                    challenge_status: progress_entry
                        .map(|progress_entry| progress_entry.challenge_status)
                        .unwrap_or(ChallengeStatus::NotStarted),
                    challenge_attempts: progress_entry
                        .map(|progress_entry| progress_entry.challenge_attempts)
                        .unwrap_or(0),
                    started: progress_entry.is_some_and(|progress_entry| progress_entry.started),
                    completed: progress_entry
                        .is_some_and(|progress_entry| progress_entry.completed),
                    requires_human: loaded.step.requires_human,
                }
            })
            .collect(),
        skipped_challenges: progress_index
            .map(|index| index.skipped_challenges(&entry.path))
            .unwrap_or(0),
        current_step_id,
        status,
        completed,
    }
}

#[cfg(test)]
mod tests {
    use super::{CourseTreeNodeView, UnitStatus, UnitView, find_unit_by_path};

    fn unit(path: &str) -> UnitView {
        UnitView {
            path: path.into(),
            title: path.into(),
            steps: Vec::new(),
            current_step_id: None,
            adapters: Vec::new(),
            status: UnitStatus::NotStarted,
            completed: false,
            skipped_challenges: 0,
        }
    }

    #[test]
    fn finds_nested_units_and_rejects_missing_paths() {
        let tree = vec![CourseTreeNodeView {
            path: "course".into(),
            title: "Course".into(),
            completed: false,
            unit: None,
            children: vec![CourseTreeNodeView {
                path: "ch".into(),
                title: "Chapter".into(),
                completed: false,
                unit: None,
                children: vec![CourseTreeNodeView {
                    path: "ch/unit".into(),
                    title: "Unit".into(),
                    completed: false,
                    unit: Some(unit("ch/unit")),
                    children: Vec::new(),
                }],
            }],
        }];

        assert_eq!(
            find_unit_by_path(&tree, "ch/unit").map(|unit| unit.path.as_str()),
            Some("ch/unit")
        );
        assert!(find_unit_by_path(&tree, "ch/missing").is_none());
    }
}
