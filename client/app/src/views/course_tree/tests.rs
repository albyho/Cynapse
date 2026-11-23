use super::{
    NodeProgress, UnitAction, node_progress, tree_items, unit_actions, unit_progress_label,
    unit_reset_description,
};
use cynapse_client_core::{
    ChallengeStatus, CourseTreeNodeView, StepView, UnitResetScope, UnitStatus, UnitView,
};
use cynapse_client_i18n::t;
use std::collections::HashSet;

fn node(path: &str, title: &str, children: Vec<CourseTreeNodeView>) -> CourseTreeNodeView {
    CourseTreeNodeView {
        path: path.to_string(),
        title: title.to_string(),
        completed: false,
        unit: None,
        children,
    }
}

fn step(id: &str) -> StepView {
    StepView {
        step_id: id.to_string(),
        title: id.to_string(),
        challenge: None,
        challenge_status: ChallengeStatus::NotStarted,
        challenge_attempts: 0,
        started: false,
        completed: false,
        requires_human: false,
    }
}

fn unit(
    status: UnitStatus,
    completed: bool,
    current_step_id: Option<&str>,
    steps: Vec<StepView>,
) -> UnitView {
    UnitView {
        path: "ch/unit".to_string(),
        title: "Unit".to_string(),
        steps,
        current_step_id: current_step_id.map(str::to_string),
        adapters: Vec::new(),
        status,
        completed,
        skipped_challenges: 0,
    }
}

#[test]
fn builds_an_expanded_course_tree_from_the_manifest_hierarchy() {
    let tree = vec![node(
        "ch1",
        "Chapter 1",
        vec![node("ch1/unit", "Unit 1", Vec::new())],
    )];
    let expanded = HashSet::from(["ch1".to_string()]);
    let items = tree_items(&tree, &expanded);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "ch1");
    assert!(items[0].is_expanded());
    assert_eq!(items[0].children[0].id, "ch1/unit");

    let collapsed = tree_items(&tree, &HashSet::new());
    assert!(!collapsed[0].is_expanded());
}

#[test]
fn unit_actions_follow_status_and_selection() {
    let none = || unit(UnitStatus::NotStarted, false, None, Vec::new());
    let unit_with_status = |status| unit(status, false, None, Vec::new());

    assert_eq!(
        unit_actions(&none(), false),
        vec![UnitAction::Select],
        "unselected untouched Unit offers selection"
    );
    assert_eq!(
        unit_actions(&unit_with_status(UnitStatus::NotStarted), true),
        vec![UnitAction::Unselect],
        "selected untouched Unit can be deselected"
    );
    assert_eq!(
        unit_actions(
            &unit(UnitStatus::InProgress, false, None, Vec::new()),
            false
        ),
        vec![UnitAction::Reset, UnitAction::Select],
        "unselected in-progress Unit can reset or select"
    );
    assert_eq!(
        unit_actions(&unit_with_status(UnitStatus::InProgress), true),
        vec![UnitAction::Reset, UnitAction::Unselect],
        "selected in-progress Unit can reset or deselect"
    );
    assert_eq!(
        unit_actions(&unit(UnitStatus::Completed, true, None, Vec::new()), true),
        vec![UnitAction::Reset],
        "a selected completed Unit stays selected and can only reset"
    );
}

#[test]
fn unit_progress_label_tracks_the_current_step() {
    let steps = || vec![step("a"), step("b")];

    assert_eq!(
        unit_progress_label(&unit(UnitStatus::NotStarted, false, None, steps())).as_deref(),
        Some("[0/2]")
    );
    assert_eq!(
        unit_progress_label(&unit(UnitStatus::InProgress, false, Some("b"), steps())).as_deref(),
        Some("[2/2]")
    );
    assert_eq!(
        unit_progress_label(&unit(UnitStatus::Completed, true, Some("b"), steps())).as_deref(),
        Some("[2/2]")
    );
}

#[test]
fn container_progress_derives_from_descendants() {
    let active_child = CourseTreeNodeView {
        unit: Some(unit(UnitStatus::InProgress, false, None, Vec::new())),
        ..node("group/child", "Child", Vec::new())
    };
    let parent = node("group", "Group", vec![active_child]);

    assert_eq!(node_progress(&parent), NodeProgress::Active);
    assert_eq!(
        node_progress(&node("empty", "Empty", Vec::new())),
        NodeProgress::Idle
    );
}

#[test]
fn completed_unit_with_unfinished_node_is_pending_finish() {
    let pending = CourseTreeNodeView {
        unit: Some(unit(UnitStatus::Completed, true, None, Vec::new())),
        ..node("group/unit", "Unit", Vec::new())
    };

    assert_eq!(node_progress(&pending), NodeProgress::PendingFinish);
}

#[test]
fn reset_description_lists_deleted_workspaces_only() {
    let description = unit_reset_description(
        &UnitResetScope {
            workspace_paths: vec!["ch/unit".to_string()],
            exclude_paths: vec!["ch/unit/child".to_string()],
        },
        false,
    );

    assert!(description.contains("workspace/ch/unit"));
    assert!(!description.contains("workspace/ch/unit/child"));
    assert!(description.contains(&t("unit.reset.workspace_files")));
}

#[test]
fn reset_description_warns_that_jupyterlab_notebook_changes_are_lost() {
    let scope = UnitResetScope {
        workspace_paths: vec!["ch/unit".to_string()],
        exclude_paths: Vec::new(),
    };

    let jupyterlab = unit_reset_description(&scope, true);
    let other = unit_reset_description(&scope, false);

    assert!(jupyterlab.contains(&t("unit.reset.jupyterlab_warning")));
    assert!(!other.contains(&t("unit.reset.jupyterlab_warning")));
}

#[test]
fn completed_node_wins_regardless_of_its_own_unit() {
    let completed = CourseTreeNodeView {
        completed: true,
        unit: Some(unit(UnitStatus::InProgress, false, None, Vec::new())),
        ..node("group/unit", "Unit", Vec::new())
    };

    assert_eq!(node_progress(&completed), NodeProgress::Done);
}
