use super::super::*;
use super::support::copied_fixture;

fn challenge_step(state: &CourseSessionState) -> StepView {
    let course = state.active_course.as_ref().unwrap();
    let unit = find_unit(&course.tree, "ch/main").unwrap();
    unit.steps
        .iter()
        .find(|step| step.step_id == "fix_add")
        .unwrap()
        .clone()
}

#[test]
fn challenge_status_survives_restart_and_clears_on_unit_reset() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("state.db");
    let (_package, package) = copied_fixture("challenge-fix-test");
    let mut session = CourseSession::new(&db).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/main", true).unwrap();
    session
        .record_challenge_failure("ch/main", "fix_add")
        .unwrap();

    let step = challenge_step(&session.state());
    assert_eq!(step.challenge_status, ChallengeStatus::Failed);
    assert_eq!(step.challenge_attempts, 1);
    drop(session);

    let mut reopened = CourseSession::new(&db).unwrap();
    reopened.open_course(&package).unwrap();
    let step = challenge_step(&reopened.state());
    assert_eq!(step.challenge_status, ChallengeStatus::Failed);
    assert_eq!(step.challenge_attempts, 1);

    let state = reopened.reset_unit("ch/main", true).unwrap().state;
    let step = challenge_step(&state);
    assert_eq!(step.challenge_status, ChallengeStatus::NotStarted);
    assert_eq!(step.challenge_attempts, 0);
}

#[test]
fn passing_or_skipping_a_challenge_completes_the_step() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("challenge-fix-test");
    session.open_course(&package).unwrap();
    session.start_unit("ch/main", true).unwrap();
    session.record_challenge_pass("ch/main", "fix_add").unwrap();
    let state = session.state();
    let step = challenge_step(&state);
    assert_eq!(step.challenge_status, ChallengeStatus::Passed);
    assert_eq!(step.challenge_attempts, 1);
    let course = state.active_course.as_ref().unwrap();
    assert_eq!(
        find_unit(&course.tree, "ch/main").unwrap().status,
        UnitStatus::Completed
    );

    let directory = tempfile::tempdir().unwrap();
    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    let (_package, package) = copied_fixture("challenge-fix-test");
    session.open_course(&package).unwrap();
    session.start_unit("ch/main", true).unwrap();
    session.record_challenge_skip("ch/main", "fix_add").unwrap();
    let state = session.state();
    let step = challenge_step(&state);
    assert_eq!(step.challenge_status, ChallengeStatus::Skipped);
    assert_eq!(step.challenge_attempts, 0);
    let course = state.active_course.as_ref().unwrap();
    let unit = find_unit(&course.tree, "ch/main").unwrap();
    assert_eq!(unit.status, UnitStatus::Completed);
    assert_eq!(unit.skipped_challenges, 1);
}
