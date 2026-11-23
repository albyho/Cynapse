//! UI-free course projections shared by Model queries and App views.

use cynapse_client_core::{
    AdapterId, CourseSessionState, CourseTreeNodeView, UnitStatus, UnitView,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitRow {
    pub path: String,
    pub title: String,
    pub status: UnitStatus,
    pub adapters: Vec<AdapterId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitProgressKind {
    New,
    Partial,
    Completed,
}

pub fn unit_progress_kind(unit: &UnitRow) -> UnitProgressKind {
    match unit.status {
        UnitStatus::Completed => UnitProgressKind::Completed,
        UnitStatus::NotStarted => UnitProgressKind::New,
        UnitStatus::InProgress | UnitStatus::Paused => UnitProgressKind::Partial,
    }
}

#[derive(Debug, Clone)]
pub struct ProjectedCourseRow {
    pub completed: bool,
    pub unit: Option<UnitView>,
    pub has_progress: bool,
}

#[derive(Debug, Default)]
pub(crate) struct CourseProjection {
    units: Vec<UnitRow>,
    rows: Arc<HashMap<String, ProjectedCourseRow>>,
    has_progress: bool,
    content_revision: u64,
    content_key: Option<String>,
    progress_revision: u64,
}

impl CourseProjection {
    pub(crate) fn refresh(&mut self, state: &CourseSessionState) {
        let content_changed = content_key(state) != self.content_key;
        if content_changed {
            self.content_key = content_key(state);
            self.content_revision = self.content_revision.wrapping_add(1);
        }
        let next_has_progress = state
            .active_course
            .as_ref()
            .is_some_and(|course| course.has_progress());
        let Some(tree) = state
            .active_course
            .as_ref()
            .map(|course| course.tree.as_slice())
        else {
            self.units.clear();
            self.rows = Arc::new(HashMap::new());
            self.has_progress = next_has_progress;
            return;
        };

        self.units.clear();
        collect_units(tree, &mut self.units);

        if content_changed || !next_has_progress {
            let mut rows = HashMap::new();
            collect_rows(tree, &mut rows);
            self.rows = Arc::new(rows);
        } else if state.progress_revision != self.progress_revision {
            let mut affected = HashSet::new();
            for path in &state.changed_unit_paths {
                add_ancestor_paths(path, &mut affected);
            }
            let rows = Arc::make_mut(&mut self.rows);
            update_rows(tree, &affected, rows);
        }
        self.progress_revision = state.progress_revision;
        self.has_progress = next_has_progress;
    }

    pub(crate) fn units(&self) -> &[UnitRow] {
        &self.units
    }

    pub(crate) fn content_revision(&self) -> u64 {
        self.content_revision
    }

    pub(crate) fn has_progress(&self) -> bool {
        self.has_progress
    }

    pub(crate) fn rows(&self) -> Arc<HashMap<String, ProjectedCourseRow>> {
        Arc::clone(&self.rows)
    }
}

fn content_key(course: &CourseSessionState) -> Option<String> {
    course.active_course.as_ref().map(|active| {
        format!(
            "{}|{}|{}",
            course.active_course_path.as_deref().unwrap_or(""),
            active.id,
            active.version
        )
    })
}

fn add_ancestor_paths(path: &str, affected: &mut HashSet<String>) {
    affected.insert(path.to_string());
    let mut current = path;
    while let Some((parent, _)) = current.rsplit_once('/') {
        affected.insert(parent.to_string());
        current = parent;
    }
}

fn update_rows(
    nodes: &[CourseTreeNodeView],
    affected: &HashSet<String>,
    rows: &mut HashMap<String, ProjectedCourseRow>,
) {
    for node in nodes {
        if affected.contains(&node.path) {
            rows.insert(
                node.path.clone(),
                ProjectedCourseRow {
                    completed: node.completed,
                    unit: node.unit.clone(),
                    has_progress: node_has_progress(node),
                },
            );
        }
        update_rows(&node.children, affected, rows);
    }
}

fn collect_rows(nodes: &[CourseTreeNodeView], rows: &mut HashMap<String, ProjectedCourseRow>) {
    for node in nodes {
        rows.insert(
            node.path.clone(),
            ProjectedCourseRow {
                completed: node.completed,
                unit: node.unit.clone(),
                has_progress: node_has_progress(node),
            },
        );
        collect_rows(&node.children, rows);
    }
}

fn node_has_progress(node: &CourseTreeNodeView) -> bool {
    node.completed
        || node
            .unit
            .as_ref()
            .is_some_and(|unit| unit.completed || unit.status != UnitStatus::NotStarted)
        || node.children.iter().any(node_has_progress)
}

fn collect_units(nodes: &[CourseTreeNodeView], rows: &mut Vec<UnitRow>) {
    for node in nodes {
        if let Some(unit) = node.unit.as_ref() {
            rows.push(UnitRow {
                path: unit.path.clone(),
                title: unit.title.clone(),
                status: unit.status,
                adapters: unit.adapters.clone(),
            });
        }
        collect_units(&node.children, rows);
    }
}
