use cynapse_client_core::course_session::validate_workspace_path;
use cynapse_client_core::{
    Course, CourseTreeNodeView, PlaybackState, UnitStatus, load_package_with_report,
};
use cynapse_client_harness::{
    AdapterScript, ScenarioMode, ScenarioOptions, ScenarioReport, ScenarioSpec, ScenarioStatus,
    run_scenario_in_place_with_options,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let mut explicit_packages = Vec::new();
    let mut ignored_paths = Vec::new();
    let mut selected_units = Vec::new();
    let mut changed_only = false;
    let mut allow_human = false;
    let mut since = "HEAD".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--course" => {
                explicit_packages.push(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--course requires a path".to_string())?,
                ));
            }
            "--ignore" => {
                ignored_paths.push(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--ignore requires a path".to_string())?,
                ));
            }
            "--unit" => {
                selected_units.push(
                    args.next()
                        .ok_or_else(|| "--unit requires a node path".to_string())?,
                );
            }
            "--changed" => changed_only = true,
            "--allow-human" => allow_human = true,
            // pnpm appends `-- <args>` after the script's own separator.
            "--" => {}
            "--since" => {
                since = args
                    .next()
                    .ok_or_else(|| "--since requires a git revision".to_string())?;
                if since.is_empty() {
                    return Err("--since must not be empty".to_string());
                }
                if since.starts_with('-') {
                    return Err("--since must not start with '-'".to_string());
                }
            }
            "--help" | "-h" => {
                println!(
                    "Usage: cynapse-content-verify [--course <path>]... [--ignore <path>]...\n\
                     \x20      [--unit <node path>]... [--changed] [--allow-human] [--since <rev>]\n\
                     Without --course, validates packages currently present under ./courses.\n\
                     --ignore applies only to discovery and does not bypass an explicit --course validation.\n\
                     Each package runs one continuous scenario covering all Units; Wait durations\n\
                     are skipped. A middle human Step fails validation because later Steps remain\n\
                     unverified; --allow-human turns that into a warning and skips the rest of the\n\
                     Unit. --unit runs only the requested Units one at a time. --changed skips a\n\
                     package when nothing changed since --since (default HEAD).\n\
                     can affect a Unit."
                );
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let selection = Selection {
        units: selected_units,
        changed_only,
        allow_human,
        since,
    };

    let cwd = Path::new(".");
    let packages = if explicit_packages.is_empty() {
        let discovered = discover_content_packages(cwd, &ignored_paths)?;
        let discovered_count = discovered.len();
        let mut ready = Vec::new();
        for package in discovered {
            if missing_narration_audio(&package) {
                println!(
                    "Skipping {} (generated narration audio is not present)",
                    package.display()
                );
            } else {
                ready.push(package);
            }
        }
        if ready.is_empty() {
            if discovered_count == 0 {
                println!("No content packages found; skipping content verification.");
            } else {
                println!(
                    "All {discovered_count} content package(s) were skipped because generated narration audio is missing; skipping content verification."
                );
            }
            return Ok(());
        }
        ready
    } else {
        explicit_packages
    };

    let artifact_root =
        std::env::temp_dir().join(format!("cynapse-content-verify-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&artifact_root)
        .map_err(|error| format!("could not create {}: {error}", artifact_root.display()))?;

    let validation = async {
        for package in packages {
            validate_package(&package, &artifact_root, &selection).await?;
            println!("Validated {}", package.display());
        }
        Ok::<(), String>(())
    }
    .await;

    let cleanup = std::fs::remove_dir_all(&artifact_root);
    match (validation, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(format!(
            "content validation succeeded but could not remove {}: {error}",
            artifact_root.display()
        )),
    }
}

fn discover_content_packages(
    cwd: &Path,
    ignored_paths: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    let ignored = ignored_paths
        .iter()
        .map(|path| {
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                cwd.join(path)
            };
            absolute.canonicalize().unwrap_or(absolute)
        })
        .collect::<Vec<_>>();
    let mut packages = Vec::new();
    let content_collections = [
        "courses", // test-isolation-allow: explicit content verifier
    ];
    for collection in content_collections {
        let root = cwd.join(collection);
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!("could not read {}: {error}", root.display()));
            }
        };
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("could not read {}: {error}", root.display()))?;
            if entry
                .file_type()
                .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?
                .is_dir()
            {
                let package = entry.path();
                let canonical_package = package.canonicalize().unwrap_or_else(|_| package.clone());
                if package.join("manifest.json").is_file()
                    && !ignored.iter().any(|ignored| ignored == &canonical_package)
                {
                    packages.push(package);
                }
            }
        }
    }
    packages.sort();
    Ok(packages)
}

/// Narration `.ogg` files are generated locally and gitignored. Discovery skips
/// packages that reference a missing one; an explicit `--course` still fails.
fn missing_narration_audio(package: &Path) -> bool {
    let Ok(bytes) = std::fs::read(package.join("manifest.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let mut paths = Vec::new();
    collect_asset_ogg_paths(&manifest, &mut paths);
    paths
        .iter()
        .any(|relative| !package.join(relative).is_file())
}

/// Only structured Asset entries are narration inputs. Titles, descriptions,
/// and environment commands may mention `.ogg` without naming an Asset.
fn collect_asset_ogg_paths(node: &serde_json::Value, paths: &mut Vec<String>) {
    if let Some(assets) = node.get("assets").and_then(serde_json::Value::as_array) {
        for asset in assets {
            if let Some(path) = asset.get("path").and_then(serde_json::Value::as_str)
                && path.ends_with(".ogg")
            {
                paths.push(path.to_string());
            }
        }
    }
    if let Some(children) = node.get("children").and_then(serde_json::Value::as_array) {
        for child in children {
            collect_asset_ogg_paths(child, paths);
        }
    }
}

#[derive(Debug, Default)]
struct Selection {
    units: Vec<String>,
    changed_only: bool,
    allow_human: bool,
    since: String,
}

/// What a set of changed files affects inside one Course Package.
#[derive(Debug, PartialEq, Eq)]
enum Affected {
    /// The change can reach every Unit: manifest, resources, assets.
    All,
    Units(std::collections::BTreeSet<String>),
}

#[derive(Debug, PartialEq, Eq)]
enum RunScope {
    /// One continuous run covers the course. It continues from the next
    /// unreached Unit if a human step stops the previous segment.
    WholeCourse,
    /// Explicit `--unit` requests run one Unit at a time for diagnosis.
    Units(Vec<String>),
    /// `--changed` found nothing that can affect a Unit.
    Skip,
}

fn select_run_scope(
    package: &Path,
    course: &Course,
    selection: &Selection,
) -> Result<RunScope, String> {
    if !selection.units.is_empty() {
        for requested in &selection.units {
            if !course.units.iter().any(|entry| &entry.path == requested) {
                return Err(format!("unknown unit {requested} in {}", package.display()));
            }
        }
        return Ok(RunScope::Units(selection.units.clone()));
    }
    if selection.changed_only {
        if !shared_contract_changes(package, &selection.since)?.is_empty() {
            return Ok(RunScope::WholeCourse);
        }
        let changed = changed_paths(package, &selection.since)?;
        let units = course
            .units
            .iter()
            .map(|entry| (entry.dir_path.clone(), entry.path.clone()))
            .collect::<Vec<_>>();
        if let Affected::Units(affected) = affected_unit_paths(&units, &changed)
            && affected.is_empty()
        {
            return Ok(RunScope::Skip);
        }
    }
    Ok(RunScope::WholeCourse)
}

/// Maps changed package files to Unit Node Paths.
///
/// A Step under `steps/<dir>/` belongs to the node that owns `<dir>` and to
/// every node below it, because a Step may be shared by a whole subtree. A Seed
/// change only affects the node with that exact Directory Path. Package-wide
/// inputs (manifest, `resources/`, assets) affect every Unit.
fn affected_unit_paths(units: &[(String, String)], changed: &[String]) -> Affected {
    let mut affected = std::collections::BTreeSet::new();
    let mut course_wide = false;
    let mut saw_relevant = false;

    for raw in changed {
        let path = raw.trim_start_matches("./");
        let (is_step, directory) = if let Some(rest) = path.strip_prefix("steps/") {
            (true, directory_of(rest))
        } else if let Some(rest) = path.strip_prefix("seeds/") {
            (false, directory_of(rest))
        } else if path == "manifest.json"
            || path.starts_with("assets/")
            || path.starts_with("resources/")
        {
            course_wide = true;
            continue;
        } else {
            continue;
        };

        saw_relevant = true;
        for (dir_path, unit_path) in units {
            let hit = if is_step {
                directory.is_empty()
                    || dir_path == &directory
                    || dir_path.starts_with(&format!("{directory}/"))
            } else {
                dir_path == &directory
            };
            if hit {
                affected.insert(unit_path.clone());
            }
        }
    }

    if course_wide {
        Affected::All
    } else {
        debug_assert!(saw_relevant || affected.is_empty());
        Affected::Units(affected)
    }
}

fn directory_of(relative: &str) -> String {
    relative
        .rsplit_once('/')
        .map(|(directory, _)| directory.to_string())
        .unwrap_or_default()
}

/// Lists files that differ from `since`, plus untracked files, relative to the
/// package root.
fn changed_paths(package: &Path, since: &str) -> Result<Vec<String>, String> {
    // `git diff` prints repo-relative paths even under `-C`, so drop the
    // package's own prefix to get package-relative names.
    let prefix = git_output(package, &["rev-parse", "--show-prefix"])?
        .into_iter()
        .next()
        .unwrap_or_default();
    let mut paths = git_output(
        package,
        &["diff", "--name-only", "--end-of-options", since, "--", "."],
    )?;
    paths.extend(git_output(
        package,
        &["ls-files", "--others", "--exclude-standard"],
    )?);
    let mut relative = paths
        .into_iter()
        .map(|path| match path.strip_prefix(&prefix) {
            Some(rest) => rest.to_string(),
            None => path,
        })
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    relative.sort();
    relative.dedup();
    Ok(relative)
}

/// Shared inputs that can invalidate every Course Package even though they sit
/// outside the package directory. `client/core` is the scenario runtime and
/// `client/harness` is the verifier itself; neither is just a build-time
/// dependency.
const SHARED_CONTRACT_PATHS: [&str; 7] = [
    "course",
    "protocol",
    "client/core/src/course_session.rs",
    "client/core/src/course_session",
    "client/core/src/server",
    "client/harness",
    "client/harness/src/bin/cynapse-content-verify.rs",
];

fn shared_contract_changes(package: &Path, since: &str) -> Result<Vec<String>, String> {
    let repo_root = git_output(package, &["rev-parse", "--show-toplevel"])?
        .into_iter()
        .next()
        .ok_or_else(|| format!("{} is not inside a git repository", package.display()))?;
    let repo_root = Path::new(&repo_root);
    let mut diff = vec!["diff", "--name-only", "--end-of-options", since, "--"];
    diff.extend(SHARED_CONTRACT_PATHS);
    let mut paths = git_output(repo_root, &diff)?;
    let mut untracked = vec!["ls-files", "--others", "--exclude-standard", "--"];
    untracked.extend(SHARED_CONTRACT_PATHS);
    paths.extend(git_output(repo_root, &untracked)?);
    Ok(paths)
}

fn git_output(package: &Path, arguments: &[&str]) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(package)
        .args(arguments)
        .output()
        .map_err(|error| format!("--changed requires git on PATH: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed in {}: {}",
            arguments.join(" "),
            package.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect())
}

async fn validate_package(
    package: &Path,
    artifact_root: &Path,
    selection: &Selection,
) -> Result<(), String> {
    let loaded = load_package_with_report(package);
    for warning in &loaded.report.warnings {
        if warning.code == "course.adapter_local_only" {
            println!("{warning}");
        } else {
            eprintln!("{warning}");
        }
    }
    if !loaded.report.errors.is_empty() {
        let errors = loaded
            .report
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  ");
        return Err(format!(
            "invalid content package {} (expected manifest.json and valid package data):\n  {errors}",
            package.display()
        ));
    }

    let course = loaded.course.ok_or_else(|| {
        format!(
            "invalid content package {} (expected manifest.json and valid package data)",
            package.display()
        )
    })?;

    if course.units.is_empty() {
        return Err(format!(
            "invalid content package {}: no units to validate",
            package.display()
        ));
    }

    let scope = select_run_scope(package, &course, selection)?;
    let run_unit_paths = match &scope {
        RunScope::WholeCourse => course
            .units
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        RunScope::Units(paths) => paths.clone(),
        RunScope::Skip => Vec::new(),
    };
    if run_unit_paths.is_empty() {
        println!(
            "No affected Units in {}; skipping scenario validation.",
            package.display()
        );
        return Ok(());
    }
    println!(
        "Validated {} (node paths: {})",
        package.display(),
        run_unit_paths.join(", ")
    );

    let workspace_root = course.workspace_path();
    let workspace_existed = workspace_root.exists();
    let resources_root = workspace_root.join("resources");
    let resources_existed = resources_root.exists();
    let unit_workspaces = run_unit_paths
        .iter()
        .filter_map(|unit_path| course.unit_workspace_path(unit_path))
        .collect::<Vec<_>>();

    let outcome = match scope {
        RunScope::WholeCourse => {
            run_whole_course(package, artifact_root, &course, selection.allow_human).await
        }
        RunScope::Units(unit_paths) => {
            run_units_by_step(
                package,
                artifact_root,
                &course,
                &unit_paths,
                selection.allow_human,
            )
            .await
        }
        RunScope::Skip => Ok(()),
    };

    validate_workspace_path(&workspace_root, &workspace_root)
        .and_then(|()| {
            unit_workspaces
                .iter()
                .try_for_each(|workspace| validate_workspace_path(&workspace_root, workspace))
        })
        .and_then(|()| validate_workspace_path(&workspace_root, &resources_root))
        .map_err(|error| format!("cannot clean generated workspace: {error}"))?;

    // Scenarios run in place, so every Unit workspace used by this run was
    // generated here. Remove them again so validation never leaves files behind
    // for the author or the next test. A pre-existing workspace root (for
    // example, one that only holds `.gitkeep`) stays in place.
    for workspace in &unit_workspaces {
        let _ = std::fs::remove_dir_all(workspace);
        remove_empty_ancestors(workspace, Some(workspace_root.as_path()));
    }
    // The per-run resource copy is derived state, not authoring content.
    // Remove it if this run created it.
    if !resources_existed {
        let _ = std::fs::remove_dir_all(&resources_root);
    }
    if !workspace_existed {
        let _ = std::fs::remove_dir_all(&workspace_root);
    }
    outcome
}

/// A headless run cannot pass a Step that requires human input. If that Step
/// has later Steps in the same Unit, report the unverified remainder instead
/// of silently jumping to a later Unit. A human final Step is a valid Unit
/// boundary: the runner reports `UnitComplete`, and the whole-course loop can
/// continue with the next Unit.
fn validate_human_boundary(report: &ScenarioReport, allow_human: bool) -> Result<(), String> {
    let position = match &report.playback_state {
        PlaybackState::StepComplete { position, .. } => position,
        PlaybackState::UnitComplete { .. } => return Ok(()),
        _ => {
            return Err(format!(
                "content validation stopped for human input without a StepComplete or UnitComplete position; artifacts: {}; reproduce: {}",
                report.artifact_path, report.repro_command
            ));
        }
    };
    if position.unit_step_index + 1 < position.unit_step_total {
        if allow_human {
            println!(
                "warning: skipping rest of Unit `{}` after human Step {}/{}; later Steps are unverified; artifacts: {}; reproduce: {}",
                position.unit_path,
                position.step_id,
                position.step_title,
                report.artifact_path,
                report.repro_command
            );
            return Ok(());
        }
        return Err(format!(
            "content validation stopped at human Step {}/{} ({} of {}); later Steps in `{}` are unverified; use --allow-human to skip the rest of the Unit; artifacts: {}; reproduce: {}",
            position.step_id,
            position.step_title,
            position.unit_step_index + 1,
            position.unit_step_total,
            position.unit_path,
            report.artifact_path,
            report.repro_command
        ));
    }
    Ok(())
}

/// Runs the whole course in one run, so `resources/` is copied once instead
/// of once per Unit.
///
/// A headless run cannot pass a Step that requires human input, so a continuous
/// run stops there. The next segment starts at the next Unit the previous
/// segment did not reach. Courses without human Steps need one segment.
async fn run_whole_course(
    package: &Path,
    artifact_root: &Path,
    course: &Course,
    allow_human: bool,
) -> Result<(), String> {
    for entry in &course.units {
        reset_unit_workspace(course, &entry.path)?;
    }

    let mut start = course.units.first().map(|entry| entry.path.clone());
    while let Some(unit_path) = start {
        let report = run_one_scenario(
            package,
            artifact_root,
            Some(unit_path.clone()),
            ScenarioMode::Continuous,
        )
        .await
        .map_err(|error| {
            format!(
                "content validation failed for {} (from {unit_path}): {error}",
                package.display()
            )
        })?;

        match report.status {
            ScenarioStatus::Completed => return Ok(()),
            ScenarioStatus::WaitingForHuman => {
                validate_human_boundary(&report, allow_human)?;
                start = next_unstarted_unit(&report, course, &unit_path);
            }
            status => {
                return Err(format!(
                    "content validation stopped for {} (from {}) with {:?}; artifacts: {}; reproduce: {}",
                    package.display(),
                    unit_path,
                    status,
                    report.artifact_path,
                    report.repro_command
                ));
            }
        }
    }
    Ok(())
}

/// Explicit `--unit` requests are diagnostic and run one Unit at a time.
async fn run_units_by_step(
    package: &Path,
    artifact_root: &Path,
    course: &Course,
    unit_paths: &[String],
    allow_human: bool,
) -> Result<(), String> {
    for unit_path in unit_paths {
        reset_unit_workspace(course, unit_path)?;
        let report = run_one_scenario(
            package,
            artifact_root,
            Some(unit_path.clone()),
            ScenarioMode::ByStep,
        )
        .await
        .map_err(|error| {
            format!(
                "content validation failed for {} / {unit_path}: {error}",
                package.display()
            )
        })?;

        match report.status {
            ScenarioStatus::Completed => {}
            ScenarioStatus::WaitingForHuman => validate_human_boundary(&report, allow_human)?,
            status => {
                return Err(format!(
                    "content validation stopped for {} / {} with {:?}; artifacts: {}; reproduce: {}",
                    package.display(),
                    unit_path,
                    status,
                    report.artifact_path,
                    report.repro_command
                ));
            }
        }
    }
    Ok(())
}

async fn run_one_scenario(
    package: &Path,
    artifact_root: &Path,
    unit_path: Option<String>,
    mode: ScenarioMode,
) -> Result<ScenarioReport, String> {
    run_scenario_in_place_with_options(
        ScenarioSpec {
            course_path: package.to_path_buf(),
            unit_path,
            mode,
            adapter_script: AdapterScript::acknowledge_all(),
            adapter_script_path: None,
            artifact_root: artifact_root.to_path_buf(),
            response_timeout: Duration::from_secs(5),
        },
        // Content verification checks that Wait is structurally valid; the
        // authored duration is presentation timing, not package correctness.
        ScenarioOptions {
            skip_waits: true,
            ..ScenarioOptions::default()
        },
    )
    .await
    .map_err(|error| error.to_string())
}

/// Finds the next Unit after the segment start that the stopped segment never
/// reached. Searching from the start prevents a skipped Unit's still-NotStarted
/// status from sending the loop backwards to the first Unit.
fn next_unstarted_unit(report: &ScenarioReport, course: &Course, start: &str) -> Option<String> {
    let mut statuses = std::collections::HashMap::new();
    if let Some(view) = report.course_state.active_course.as_ref() {
        collect_unit_statuses(&view.tree, &mut statuses);
    }
    let start_index = course.units.iter().position(|entry| entry.path == start)?;
    course
        .units
        .iter()
        .skip(start_index + 1)
        .find(|entry| statuses.get(&entry.path) == Some(&UnitStatus::NotStarted))
        .map(|entry| entry.path.clone())
}

fn collect_unit_statuses(
    nodes: &[CourseTreeNodeView],
    statuses: &mut std::collections::HashMap<String, UnitStatus>,
) {
    for node in nodes {
        if let Some(unit) = node.unit.as_ref() {
            statuses.insert(unit.path.clone(), unit.status);
        }
        collect_unit_statuses(&node.children, statuses);
    }
}

/// Removes directories made empty by deleting a Unit workspace, stopping at the
/// package workspace root.
fn remove_empty_ancestors(directory: &Path, stop: Option<&Path>) {
    let Some(stop) = stop else {
        return;
    };
    let mut current = directory.parent();
    while let Some(path) = current {
        if path == stop || !path.starts_with(stop) {
            break;
        }
        if std::fs::remove_dir(path).is_err() {
            break;
        }
        current = path.parent();
    }
}

/// Scenarios run in place, so each Unit must start from a clean workspace:
/// remove whatever a previous run or the author left behind for that Unit.
fn reset_unit_workspace(course: &Course, unit_path: &str) -> Result<(), String> {
    let Some(workspace) = course.unit_workspace_path(unit_path) else {
        return Ok(());
    };
    validate_workspace_path(&course.workspace_path(), &workspace).map_err(|error| {
        format!(
            "cannot reset workspace {} for {unit_path}: {error}",
            workspace.display()
        )
    })?;
    match std::fs::remove_dir_all(&workspace) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "cannot reset workspace {} for {}: {error}",
            workspace.display(),
            unit_path
        )),
    }
}

#[cfg(test)]
#[path = "cynapse-content-verify/tests.rs"]
mod tests;
