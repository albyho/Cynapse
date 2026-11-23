use super::super::*;
use super::support::copied_fixture;

#[test]
fn course_view_has_path_addressed_units() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("multi-node-demo");
    session.open_course(package).unwrap();
    let state = session.start_unit("ch_foundation/main", true).unwrap();
    let course = state.active_course.unwrap();

    assert_eq!(course.tree.len(), 8);
    assert_eq!(
        find_unit(&course.tree, "ch_foundation/main")
            .unwrap()
            .status,
        UnitStatus::NotStarted
    );
}

#[test]
fn starting_a_unit_keeps_the_opened_package_active() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("multi-node-demo");
    session.open_course(&package).unwrap();
    session.start_unit("ch_foundation/main", true).unwrap();

    assert_eq!(
        session.active_course().unwrap().package_root,
        super::super::normalize_package_path(&package).unwrap()
    );
}

#[test]
fn unit_view_exposes_its_effective_adapter_requirements() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("manual-boundary-demo");
    session.open_course(package).unwrap();
    let state = session.start_unit("ch/unit", true).unwrap();
    let course = state.active_course.unwrap();

    assert!(
        find_unit(&course.tree, "ch/unit")
            .unwrap()
            .adapters
            .is_empty()
    );
}

#[test]
fn selecting_a_unit_restores_its_position() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("independent-units");
    session.open_course(package).unwrap();

    session.start_unit("ch/unit_c", true).unwrap();
    session
        .record_progress("ch/unit_c", "shared_1_node_unit_c", true)
        .unwrap();
    let state = session.start_unit("ch/unit_d", true).unwrap();
    let course = state.active_course.unwrap();

    assert_eq!(course.current_unit_path.as_deref(), Some("ch/unit_d"));
    let previous = find_unit(&course.tree, "ch/unit_c").unwrap();
    assert_eq!(previous.status, UnitStatus::Completed);
    assert!(
        previous.steps.iter().all(|step| step.completed),
        "starting another Unit must preserve the previous Unit's Step progress"
    );
}

#[test]
fn resetting_a_completed_unit_leaves_it_unselected_until_reselected() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("independent-units");
    session.open_course(package).unwrap();
    session.start_unit("ch/unit_c", true).unwrap();
    session
        .record_progress("ch/unit_c", "shared_1_node_unit_c", true)
        .unwrap();

    let reset = session.reset_unit("ch/unit_c", true).unwrap();
    assert!(
        reset.state.selected_unit.is_none(),
        "reset runs before selection and leaves no Unit selected"
    );
    assert_eq!(
        find_unit(&reset.state.active_course.unwrap().tree, "ch/unit_c")
            .unwrap()
            .status,
        UnitStatus::NotStarted
    );

    let selected = session.select_unit("ch/unit_c").unwrap();
    assert_eq!(
        selected.selected_unit.map(|unit| unit.path).as_deref(),
        Some("ch/unit_c"),
        "selection happens only after the reset succeeded"
    );
}

#[test]
fn clearing_unit_selection_keeps_progress() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("independent-units");
    session.open_course(package).unwrap();
    session.start_unit("ch/unit_c", true).unwrap();

    let state = session.clear_selection().unwrap();
    assert!(state.selected_unit.is_none());
    let course = state.active_course.expect("the course stays open");
    assert!(!course.has_progress());
    assert!(course.current_unit_path.is_none());
    assert!(course.current_step_id.is_none());

    let state = session.select_unit("ch/unit_c").unwrap();
    assert_eq!(
        state.selected_unit.map(|unit| unit.path).as_deref(),
        Some("ch/unit_c")
    );
}

#[test]
fn completing_a_unit_clears_the_selected_unit() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    let (_package, package) = copied_fixture("independent-units");
    session.open_course(package).unwrap();
    session.start_unit("ch/unit_c", true).unwrap();

    session
        .record_progress("ch/unit_c", "shared_1_node_unit_c", true)
        .unwrap();

    let state = session.state();
    assert!(state.selected_unit.is_none());
    let course = state.active_course.expect("the course stays open");
    let unit = find_unit(&course.tree, "ch/unit_c").expect("the completed unit");
    assert_eq!(unit.status, UnitStatus::Completed);
}

#[test]
fn completing_the_only_unit_clears_the_selected_unit() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("state.db");
    let mut session = CourseSession::new(&db_path).unwrap();
    let (_package, package) = copied_fixture("multi-step-demo");
    session.open_course(package).unwrap();
    session.start_unit("ch_multi_step/main", true).unwrap();

    session
        .record_progress("ch_multi_step/main", "write_first", true)
        .unwrap();
    assert!(session.state().selected_unit.is_some());

    session
        .record_progress("ch_multi_step/main", "write_second", true)
        .unwrap();
    let state = session.state();
    assert!(state.selected_unit.is_none());
    let course = state.active_course.expect("the course stays open");
    let unit = find_unit(&course.tree, "ch_multi_step/main").expect("the completed unit");
    assert_eq!(unit.status, UnitStatus::Completed);
    assert_eq!(course.current_unit_path, None);

    // 重启后也不再把已完成的课程默认选中到第一个 Unit。
    let reopened = CourseSession::new(&db_path).unwrap();
    assert!(reopened.state().selected_unit.is_none());
}

#[test]
fn course_has_progress_is_derived_from_unit_states() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("manual-boundary-demo");
    session.open_course(package).unwrap();
    assert!(!session.state().active_course.unwrap().has_progress());

    session.start_unit("ch/unit", true).unwrap();
    assert!(
        !session.state().active_course.unwrap().has_progress(),
        "selection alone is not progress"
    );

    session
        .record_progress("ch/unit", "introduce", false)
        .unwrap();
    assert!(session.state().active_course.unwrap().has_progress());

    session
        .record_progress("ch/unit", "introduce", true)
        .unwrap();
    session.record_progress("ch/unit", "manual", true).unwrap();
    session
        .record_progress("ch/unit", "continue", true)
        .unwrap();
    assert!(session.state().active_course.unwrap().has_progress());
}

#[test]
fn selected_unit_is_the_same_view_as_the_course_tree() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("independent-units");
    session.open_course(package).unwrap();
    session.start_unit("ch/unit_c", true).unwrap();

    let state = session.state();
    let selected = state.selected_unit.expect("the selected unit");
    let course = state.active_course.expect("the active course");
    let in_tree = find_unit(&course.tree, &selected.path).expect("the unit in the tree");
    assert_eq!(selected, in_tree);
}
