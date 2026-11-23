//! Canonical manifest serialization.
//!
//! The writer must round-trip the model and emit one stable field order instead
//! of whatever order the author happened to use.

use super::*;

fn document() -> serde_json::Value {
    serde_json::json!({
        "id": "course",
        "title": "Course",
        "version": "1.2.3",
        "workspace": "workspace",
        "adapters": ["vscode"],
        "environment": [{
            "id": "rustc",
            "command": "rustc --version",
            "minimum_version": "1.85.0",
            "install_hint": "install Rust"
        }],
        "children": [{
            "id": "ch",
            "title": "Chapter",
            "dir_name": "chapter",
            "adapters": null,
            "environment": [],
            "steps": [{ "step_file": "steps/chapter/one.json" }],
            "assets": [{ "id": "a", "path": "assets/a.ogg" }],
            "children": [{
                "id": "sub",
                "title": "Sub",
                "children": [{
                    "id": "leaf",
                    "title": "Leaf",
                    "steps": [{ "step_file": "steps/chapter/leaf/one.json" }]
                }]
            }]
        }]
    })
}

#[test]
fn canonical_json_round_trips_the_manifest() {
    let manifest: Manifest = serde_json::from_value(document()).unwrap();
    let rendered = canonical_manifest_json(&manifest).unwrap();
    let reparsed: Manifest = serde_json::from_str(&rendered).unwrap();
    let rendered_again = canonical_manifest_json(&reparsed).unwrap();

    assert_eq!(rendered, rendered_again);
    assert!(rendered.ends_with('\n'));
}

#[test]
fn canonical_json_uses_the_documented_field_order() {
    let manifest: Manifest = serde_json::from_value(document()).unwrap();
    let rendered = canonical_manifest_json(&manifest).unwrap();

    let position = |key: &str| {
        rendered
            .find(&format!("\"{key}\""))
            .unwrap_or_else(|| panic!("missing key {key}"))
    };

    assert!(position("id") < position("title"));
    assert!(position("title") < position("version"));
    assert!(position("version") < position("workspace"));
    assert!(position("workspace") < position("adapters"));
    assert!(position("adapters") < position("environment"));
    assert!(position("environment") < position("children"));

    // Node order: id, title, dir_name, adapters, environment, steps, assets,
    // children.
    let node = serde_json::to_string_pretty(&manifest.children[0]).unwrap();
    let node_position = |key: &str| {
        node.find(&format!("\"{key}\""))
            .unwrap_or_else(|| panic!("missing node key {key}"))
    };
    assert!(node_position("id") < node_position("title"));
    assert!(node_position("title") < node_position("dir_name"));
    assert!(node_position("dir_name") < node_position("adapters"));
    assert!(node_position("adapters") < node_position("environment"));
    assert!(node_position("environment") < node_position("steps"));
    assert!(node_position("steps") < node_position("assets"));
    assert!(node_position("assets") < node_position("children"));
}

#[test]
fn absent_optional_fields_stay_absent() {
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "id": "course",
        "title": "Course",
        "children": [{
            "id": "unit",
            "title": "Unit",
            "steps": [{ "step_file": "steps/unit.json" }]
        }]
    }))
    .unwrap();

    let rendered = canonical_manifest_json(&manifest).unwrap();

    assert!(!rendered.contains("\"dir_name\""));
    assert!(!rendered.contains("\"adapters\""));
    assert!(!rendered.contains("\"environment\""));
    assert!(!rendered.contains("\"assets\""));
    assert!(!rendered.contains("\"version\""));
}

#[test]
fn explicit_requirement_overrides_survive_the_round_trip() {
    let manifest: Manifest = serde_json::from_value(document()).unwrap();
    let rendered = canonical_manifest_json(&manifest).unwrap();

    assert!(rendered.contains("\"adapters\": null"));
    assert!(rendered.contains("\"environment\": null"));
}
