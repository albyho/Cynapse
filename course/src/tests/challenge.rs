use super::*;

fn challenge_step() -> serde_json::Value {
    serde_json::json!({
        "step_id": "write_hello",
        "title": "Fix the challenge",
        "actions": [{
            "type": "TerminalEnsure",
            "session_key": "main",
            "cwd": "."
        }],
        "challenge": {
            "instructions": "Fix the failing test.",
            "checks": [{
                "type": "COMMAND",
                "session_key": "main",
                "command": "node --test"
            }]
        }
    })
}

fn load_error(step: serde_json::Value) -> String {
    let package = write_test_package("steps/write_hello.json", step);
    load_package(package.path()).unwrap_err().to_string()
}

#[test]
fn challenge_step_uses_the_documented_defaults() {
    let package = write_test_package("steps/write_hello.json", challenge_step());
    let course = load_package(package.path()).unwrap();

    let challenge = course.challenge("ch/main", "write_hello").unwrap();
    assert_eq!(challenge.instructions, "Fix the failing test.");
    assert!(challenge.allow_skip);
    assert_eq!(challenge.hint, None);
    assert_eq!(challenge.checks.len(), 1);
    assert_eq!(challenge.checks[0].cwd(), ".");
    assert_eq!(challenge.checks[0].accepted_exit_codes(), &[0]);
    assert_eq!(challenge.checks[0].timeout_ms(), 300_000);

    let playback = course.playback_steps();
    assert!(playback[0].requires_human);
    assert_eq!(playback[0].step_id, "write_hello");
}

#[test]
fn challenge_step_requires_exactly_one_check() {
    let mut step = challenge_step();
    step["challenge"]["checks"] = serde_json::json!([
        { "type": "COMMAND", "session_key": "main", "command": "node --test" },
        { "type": "COMMAND", "session_key": "main", "command": "node --test" }
    ]);

    let error = load_error(step);
    assert!(error.contains("exactly one COMMAND check"), "{error}");
}

#[test]
fn challenge_check_requires_a_matching_terminal_session() {
    let mut step = challenge_step();
    step["challenge"]["checks"][0]["session_key"] = serde_json::json!("other");

    let error = load_error(step);
    assert!(error.contains("no matching TerminalEnsure"), "{error}");
}

#[test]
fn challenge_check_rejects_a_cwd_outside_the_workspace() {
    let mut step = challenge_step();
    step["challenge"]["checks"][0]["cwd"] = serde_json::json!("../outside");

    let error = load_error(step);
    assert!(
        error.contains("must be inside the Unit Workspace"),
        "{error}"
    );
}

#[test]
fn challenge_check_rejects_empty_command() {
    let mut step = challenge_step();
    step["challenge"]["checks"][0]["command"] = serde_json::json!("  ");

    let error = load_error(step);
    assert!(error.contains("command must not be empty"), "{error}");
}

#[test]
fn challenge_check_rejects_empty_exit_codes() {
    let mut step = challenge_step();
    step["challenge"]["checks"][0]["accepted_exit_codes"] = serde_json::json!([]);

    let error = load_error(step);
    assert!(
        error.contains("accepted_exit_codes must not be empty"),
        "{error}"
    );
}

#[test]
fn challenge_check_rejects_zero_timeout() {
    let mut step = challenge_step();
    step["challenge"]["checks"][0]["timeout_ms"] = serde_json::json!(0);

    let error = load_error(step);
    assert!(
        error.contains("timeout_ms must be greater than zero"),
        "{error}"
    );
}

#[test]
fn challenge_check_rejects_a_timeout_over_the_adapter_deadline() {
    let mut step = challenge_step();
    step["challenge"]["checks"][0]["timeout_ms"] = serde_json::json!(1_800_001);

    let error = load_error(step);
    assert!(
        error.contains("timeout_ms must not exceed 1800000 ms"),
        "{error}"
    );
}

#[test]
fn challenge_rejects_empty_instructions_and_hint() {
    let mut step = challenge_step();
    step["challenge"]["instructions"] = serde_json::json!(" ");
    let error = load_error(step);
    assert!(error.contains("instructions must not be empty"), "{error}");

    let mut step = challenge_step();
    step["challenge"]["hint"] = serde_json::json!(" ");
    let error = load_error(step);
    assert!(error.contains("hint must not be empty"), "{error}");
}

#[test]
fn challenge_rejects_empty_expected_output_entries() {
    let mut step = challenge_step();
    step["challenge"]["checks"][0]["expected_output"] = serde_json::json!([""]);

    let error = load_error(step);
    assert!(
        error.contains("expected_output must not contain empty strings"),
        "{error}"
    );
}

#[test]
fn challenge_allows_an_explicit_skip_policy() {
    let mut step = challenge_step();
    step["challenge"]["allow_skip"] = serde_json::json!(false);

    let package = write_test_package("steps/write_hello.json", step);
    let course = load_package(package.path()).unwrap();
    let challenge = course.challenge("ch/main", "write_hello").unwrap();
    assert!(!challenge.allow_skip);
}
