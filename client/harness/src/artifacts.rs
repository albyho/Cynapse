use super::*;

pub(super) fn write_artifacts(path: &Path, report: &ScenarioReport) -> Result<(), ScenarioError> {
    fs::write(path.join("report.json"), serde_json::to_vec_pretty(report)?)?;
    let events = report
        .events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    fs::write(path.join("events.jsonl"), format!("{events}\n"))?;
    Ok(())
}

pub(super) fn repro_command(spec: &ScenarioSpec) -> String {
    let mut command = format!(
        "cargo run -p cynapse-client-harness --bin cynapse-scenario -- --course \"{}\"",
        spec.course_path.display(),
    );
    if let Some(unit_path) = spec.unit_path.as_deref() {
        command.push_str(&format!(" --unit \"{unit_path}\""));
    }
    let mode = match spec.mode {
        ScenarioMode::ByStep => "by-step",
        ScenarioMode::Continuous => "continuous",
        ScenarioMode::StopAfterStep => "stop-after-step",
    };
    command.push_str(&format!(" --mode {mode}"));
    command.push_str(&format!(
        " --artifact-root \"{}\" --timeout-ms {}",
        spec.artifact_root.display(),
        spec.response_timeout.as_millis(),
    ));
    if let Some(script_path) = spec.adapter_script_path.as_deref() {
        command.push_str(&format!(" --adapter-script \"{}\"", script_path.display()));
    }
    command
}

pub(super) fn cleanup_command(path: &Path) -> String {
    if cfg!(windows) {
        format!(
            "Remove-Item -LiteralPath \"{}\" -Recurse -Force",
            path.display()
        )
    } else {
        format!("rm -rf -- '{}'", path.display())
    }
}
