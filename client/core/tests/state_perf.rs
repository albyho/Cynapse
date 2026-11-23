//! Release-only state budget check for `#96` / ADR-0060.
//!
//! Run with `pnpm verify:client:perf`. The default test gate ignores this
//! because it builds a 3,000-Step package and must run in release mode.
//!
//! The old envelope payload budget belonged to the previous IPC host; the GPUI
//! host reads reducer state directly, so this benchmark measures state building
//! and reducer application only.

use cynapse_client_core::{
    ClientEvent, ClientStateReducer, ConnectionState, CourseSession, LearningView, PlaybackState,
};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const UNITS: usize = 1_000;
const STEPS_PER_UNIT: usize = 3;
const STEP_BUDGET: Duration = Duration::from_millis(5);
const FAIL_CEILING: Duration = Duration::from_millis(50);

fn write_course(root: &Path) {
    fs::create_dir_all(root.join("steps")).unwrap();
    let units = (0..UNITS)
        .map(|unit| {
            let steps = (0..STEPS_PER_UNIT)
                .map(|step| {
                    let file = format!("steps/s{unit}-{step}.json");
                    fs::write(
                        root.join(&file),
                        serde_json::to_vec(&serde_json::json!({
                            "step_id": format!("s{unit}-{step}"),
                            "title": format!("Step {unit}-{step}"),
                            "actions": [{"type": "Present"}]
                        }))
                        .unwrap(),
                    )
                    .unwrap();
                    serde_json::json!({"step_file": file})
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": format!("u{unit}"),
                "title": format!("Unit {unit}"),
                "steps": steps
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": "state_budget",
            "title": "3,000-Step State Budget",
            "children": [{"id": "ch", "title": "Chapter", "children": units}]
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "3,000-Step release benchmark; run pnpm verify:client:perf"]
fn three_thousand_step_state_budget() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    write_course(&package);

    let mut session = CourseSession::new(directory.path().join("state.db")).unwrap();
    session.open_course(&package).unwrap();
    session.start_unit("ch/u0", true).unwrap();

    let samples = 10;
    let started = Instant::now();
    for _ in 0..samples {
        std::hint::black_box(session.state());
    }
    let average = started.elapsed() / samples;

    let state = session.state();
    let mut reducer = ClientStateReducer::new(
        state.clone(),
        LearningView::default(),
        PlaybackState::default(),
        ConnectionState::default(),
    );
    let mut changed = state;
    let active = changed.active_course.as_mut().expect("active course");
    active.current_step_id = Some("s0-1".to_string());
    let apply_started = Instant::now();
    reducer.apply(&[ClientEvent::Course(Arc::new(changed))]);
    let apply_elapsed = apply_started.elapsed();
    let combined = average + apply_elapsed;

    println!(
        "3,000-Step state: {average:?}/build, apply {apply_elapsed:?}, combined {combined:?} (target {STEP_BUDGET:?})"
    );
    assert!(
        average <= FAIL_CEILING,
        "state build {average:?} exceeded the {FAIL_CEILING:?} regression ceiling"
    );
    if combined > STEP_BUDGET {
        eprintln!(
            "warning: combined state+apply {combined:?} is above the {STEP_BUDGET:?} target; see the issue #96 follow-up"
        );
    }
}
