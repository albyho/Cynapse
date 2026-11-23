//! Package validity checks shared by the loader and `verify:content`.

use super::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageReport {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

impl PackageReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// A package loaded once, together with every validity finding.
///
/// `course` is present only when the report has no errors. Structural errors
/// retain their original `CourseError` for the strict loader.
#[derive(Debug)]
pub struct PackageLoad {
    pub course: Option<Course>,
    pub report: PackageReport,
    error: Option<CourseError>,
}

impl PackageLoad {
    /// Convert the loaded package back to the strict loader result.
    pub fn into_result(self) -> Result<Course, CourseError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        match self.course {
            Some(course) => Ok(course),
            None => Err(CourseError::Package(
                self.report
                    .errors
                    .first()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "course package validation failed".to_string()),
            )),
        }
    }
}

/// Load and check a package once, returning both the report and the Course
/// when it is valid.
pub fn load_package_with_report(root: &Path) -> PackageLoad {
    match loader::load_package_unchecked(root) {
        Ok(course) => {
            let report = check_course(&course);
            PackageLoad {
                course: report.is_ok().then_some(course),
                report,
                error: None,
            }
        }
        Err(error) => {
            let diagnostic = Diagnostic::from(&error);
            PackageLoad {
                course: None,
                report: PackageReport {
                    errors: vec![diagnostic],
                    warnings: Vec::new(),
                },
                error: Some(error),
            }
        }
    }
}

/// Check every semantic rule without stopping at the first finding.
///
/// A structural load failure is reported as one `course.package` diagnostic;
/// semantic rules are collected for a structurally loadable package.
pub fn check_package(root: &Path) -> PackageReport {
    load_package_with_report(root).report
}

/// Check an already-loaded Course without re-reading the package.
pub fn check_course(course: &Course) -> PackageReport {
    let mut report = PackageReport::default();
    let (caption_errors, opening_warnings) = narration_findings(course);
    report.errors.extend(caption_errors);
    report.warnings.extend(opening_warnings);

    for entry in &course.units {
        if course.unit_has_invalid_adapter_configuration(&entry.path) {
            let mut causes = Vec::new();
            if course.unit_requires_app_adapter(&entry.path) {
                causes.push("has non-Present actions".to_string());
            }
            if !entry.unit.environment.is_empty() {
                let ids = entry
                    .unit
                    .environment
                    .iter()
                    .map(|requirement| requirement.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                causes.push(format!("declares environment checks ({ids})"));
            }
            report.errors.push(
                Diagnostic::error(
                    "course.adapter_configuration",
                    format!(
                        "invalid adapters/environment configuration: unit {} {} but resolves empty `adapters`; omit `adapters` (and `environment`) so it inherits the course requirements instead of clearing them",
                        entry.path,
                        causes.join(" and ")
                    ),
                )
                .at_node(&entry.path),
            );
        } else if entry.unit.adapters.is_empty() {
            report.warnings.push(
                Diagnostic::warning(
                    "course.adapter_local_only",
                    format!(
                        "Local-only units: unit {} needs no external App Adapter",
                        entry.path
                    ),
                )
                .at_node(&entry.path),
            );
        }
    }

    report
}

fn narration_findings(course: &Course) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let step_files: HashMap<(&str, &str), &Path> = course
        .steps
        .iter()
        .map(|step| {
            (
                (step.unit_path.as_str(), step.step.step_id.as_str()),
                step.step_file.as_path(),
            )
        })
        .collect();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for step in course.playback_steps() {
        let step_file = step_files
            .get(&(step.unit_path.as_str(), step.step_id.as_str()))
            .map(|path| path.to_string_lossy().into_owned());
        for (action_index, action) in step.actions.iter().enumerate() {
            for (phase, clips) in [
                ("before", action.narration.before.as_ref()),
                ("during", action.narration.during.as_ref()),
                ("after", action.narration.after.as_ref()),
            ] {
                let Some(clips) = clips else {
                    continue;
                };
                for (clip_index, clip) in clips.clips.iter().enumerate() {
                    if let Some(caption) = clip.caption.as_deref()
                        && let Err(error) = validate_caption_markup(caption)
                    {
                        let diagnostic = Diagnostic::error(
                            "course.caption_markup",
                            format!(
                                "invalid caption markup: {} / step {} / action {action_index} / {phase} clip {clip_index}: {error}",
                                step.unit_path, step.step_id
                            ),
                        )
                        .at_node(&step.unit_path)
                        .at_pointer(format!(
                            "/actions/{action_index}/narration/{phase}/clips/{clip_index}/caption"
                        ));
                        errors.push(match step_file.as_deref() {
                            Some(step_file) => diagnostic.at_step_file(step_file),
                            None => diagnostic,
                        });
                    }

                    if let Some(action_name) = opening_action_name(&action.action)
                        && clip.audio_path.is_none()
                        && clip.duration_ms > 0
                    {
                        let diagnostic = Diagnostic::warning(
                            "narration.opening_caption_only",
                            format!(
                                "{} / step {} / action {action_index} ({action_name}) / {phase} clip {clip_index} is caption-only; add an audio target if narration is intended",
                                step.unit_path, step.step_id
                            ),
                        )
                        .at_node(&step.unit_path);
                        warnings.push(match step_file.as_deref() {
                            Some(step_file) => diagnostic.at_step_file(step_file),
                            None => diagnostic,
                        });
                    }
                }
            }
        }
    }

    (errors, warnings)
}

fn opening_action_name(action: &Action) -> Option<&'static str> {
    match action {
        Action::OpenPreview(_) => Some("OpenPreview"),
        Action::OpenFileWithPreview(_) => Some("OpenFileWithPreview"),
        _ => None,
    }
}
