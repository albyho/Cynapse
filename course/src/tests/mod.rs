//! Course module tests by sub-domain. Shared fixtures
//! (`write_test_package`, `self_contained_step`, ...) live below.
//!
//! Submodules:
//! - `manifest` — manifest parsing and rejection cases
//! - `tree` — course tree structure and node rules
//! - `actions` — Action definition validation
//! - `challenge` — Challenge Step and Check validation
//! - `narration` — narration/audio asset resolution
//! - `requirements` — requirement overrides and validation
//! - `workspace` — workspace seed behavior
//! - `adapters` — adapter requirement metadata
//! - `check` — shared package validity report
//! - `edit_ops` — pure code-edit semantics shared with the Client scenario harness
//! - `file_ops` — filesystem Action policy shared with the Client scenario harness
//! - package builders used by the suites above

use super::loader::{build_course_root, collect_units};
use super::*;

mod actions;
mod adapters;
mod challenge;
mod check;
mod dir_name;
mod edit_ops;
mod file_ops;
mod fixtures;
mod manifest;
mod narration;
mod requirements;
mod serialize;
mod speech;
mod tree;
mod workspace;

// Shared test-support helpers used across every domain module.

fn narrate(phase: &str) -> serde_json::Value {
    serde_json::json!({
        "asset_id": "narration",
        "clips": [{
            "offset_ms": 0,
            "duration_ms": 1000,
            "caption": format!("{phase} caption")
        }]
    })
}

fn add_unit_audio_asset(package: &tempfile::TempDir, id: &str, relative_path: &str) {
    let manifest_path = package.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let assets = manifest["children"][0]["children"][0]
        .as_object_mut()
        .unwrap()
        .entry("assets")
        .or_insert_with(|| serde_json::json!([]));
    assets
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id": id, "path": relative_path}));
    std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let destination = package.path().join(relative_path);
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/narration-demo/assets/delayed-intro.ogg"),
        destination,
    )
    .unwrap();
}

fn write_test_package(step_file: &str, step_document: serde_json::Value) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "self_contained",
            "title": "Self Contained",
            "adapters": ["vscode"],
        "children": [{
                    "id": "ch",
                    "title": "Grouping",
                    "children": [{
                        "id": "main",
                        "title": "Main Unit",
                        "steps": [{ "step_file": step_file }]
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let path = directory.path().join(step_file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_vec_pretty(&step_document).unwrap()).unwrap();
    directory
}

fn self_contained_step() -> serde_json::Value {
    serde_json::json!({
        "step_id": "write_hello",
        "title": "Write Hello",
        "actions": [{
            "type": "CreateFile",
            "path": "hello.txt"
        }]
    })
}
