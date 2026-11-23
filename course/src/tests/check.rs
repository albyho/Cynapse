//! Package check module tests.

use super::*;

#[test]
fn check_package_reports_caption_markup_errors() {
    let mut step = self_contained_step();
    step["actions"][0]["narration"] = serde_json::json!({
        "after": {
            "clips": [{
                "duration_ms": 1000,
                "caption": "<c.aqua>unclosed"
            }]
        }
    });
    let package = write_test_package("steps/write_hello.json", step);

    let report = check_package(package.path());

    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "course.caption_markup"),
        "{report:?}"
    );
    assert!(load_package(package.path()).is_err());
}

#[test]
fn check_package_reports_invalid_adapter_configuration() {
    let package = write_test_package("steps/write_hello.json", self_contained_step());
    disable_unit_adapters(&package);

    let report = check_package(package.path());

    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "course.adapter_configuration")
    );
    assert!(load_package(package.path()).is_err());
}

#[test]
fn load_package_with_report_returns_a_course_only_when_clean() {
    let valid = write_test_package("steps/write_hello.json", self_contained_step());
    let loaded = load_package_with_report(valid.path());
    assert!(loaded.report.is_ok());
    assert!(loaded.course.is_some());

    let invalid = write_test_package("steps/write_hello.json", self_contained_step());
    disable_unit_adapters(&invalid);
    let loaded = load_package_with_report(invalid.path());
    assert!(
        loaded
            .report
            .errors
            .iter()
            .any(|error| error.code == "course.adapter_configuration")
    );
    assert!(loaded.course.is_none());
}

#[test]
fn check_package_reports_opening_caption_only_narration() {
    let step = serde_json::json!({
        "step_id": "opening",
        "title": "Opening",
        "actions": [{
            "type": "OpenFileWithPreview",
            "path": "INTRO.md",
            "narration": {
                "after": {
                    "clips": [{
                        "duration_ms": 4000,
                        "caption": "opening"
                    }]
                }
            }
        }]
    });
    let package = write_test_package("steps/opening.json", step);

    let report = check_package(package.path());

    assert!(report.warnings.iter().any(|warning| {
        warning.code == "narration.opening_caption_only"
            && warning.message.contains("OpenFileWithPreview")
    }));
}

#[test]
fn check_package_reports_local_only_units() {
    let step = serde_json::json!({
        "step_id": "present",
        "title": "Present",
        "actions": [{"type": "Present"}]
    });
    let package = write_test_package("steps/present.json", step);
    disable_unit_adapters(&package);

    let report = check_package(package.path());

    assert!(report.errors.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.code == "course.adapter_local_only")
    );
}

fn disable_unit_adapters(package: &tempfile::TempDir) {
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["children"][0]["children"][0]["adapters"] = serde_json::json!([]);
    std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}
