use cynapse_client_harness::{
    AdapterScript, ScenarioMode, ScenarioSpec, ScenarioStatus, run_scenario,
};
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("scenario report is serializable")
            );
            if report.artifacts_retained {
                eprintln!("scenario artifacts retained at {}", report.artifact_path);
                eprintln!("reproduce with: {}", report.repro_command);
                eprintln!("cleanup with: {}", report.cleanup_command);
            }
            if report.status != ScenarioStatus::Completed {
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<cynapse_client_harness::ScenarioReport, Box<dyn std::error::Error>> {
    let mut course = None;
    let mut unit = None;
    let mut mode = ScenarioMode::ByStep;
    let mut adapter_script = AdapterScript::acknowledge_all();
    let mut adapter_script_path = None;
    let mut artifact_root = std::env::temp_dir().join("cynapse-scenarios");
    let mut response_timeout = Duration::from_secs(5);
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--course" => course = args.next().map(PathBuf::from),
            "--unit" => {
                let value = args.next().ok_or("--unit requires a node path")?;
                if value.starts_with("--") {
                    return Err("--unit requires a node path".into());
                }
                unit = Some(value);
            }
            "--mode" => {
                let value = args.next().ok_or("--mode requires a value")?;
                mode = match value.as_str() {
                    "by-step" | "by_step" => ScenarioMode::ByStep,
                    "continuous" => ScenarioMode::Continuous,
                    "stop-after-step" | "stop_after_step" => ScenarioMode::StopAfterStep,
                    _ => return Err(format!("unsupported scenario mode: {value}").into()),
                };
            }
            "--adapter-script" => {
                let path = args.next().ok_or("--adapter-script requires a path")?;
                adapter_script_path = Some(PathBuf::from(&path));
                adapter_script = serde_json::from_slice(&std::fs::read(path)?)?;
            }
            "--artifact-root" => {
                artifact_root =
                    PathBuf::from(args.next().ok_or("--artifact-root requires a path")?);
            }
            "--timeout-ms" => {
                let value = args.next().ok_or("--timeout-ms requires a value")?;
                response_timeout = Duration::from_millis(value.parse()?);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: cynapse-scenario --course <path> [--unit <node path>] [--mode by-step|continuous|stop-after-step] [--adapter-script <json>] [--artifact-root <path>] [--timeout-ms <ms>]"
                );
                std::process::exit(0);
            }
            other => {
                return Err(format!(
                    "unknown argument: {other}; use --unit <node path> to select a run target"
                )
                .into());
            }
        }
    }
    std::fs::create_dir_all(&artifact_root)?;
    Ok(run_scenario(ScenarioSpec {
        course_path: course.ok_or("--course is required")?,
        unit_path: unit,
        mode,
        adapter_script,
        adapter_script_path,
        artifact_root,
        response_timeout,
    })
    .await?)
}
