//! `dir_name` / Directory Path behavior.
//!
//! Node Path (ids) is identity; Directory Path (dir names) is layout. These
//! tests pin the split: layout follows `dir_name`, progress keys never do.

use super::*;

fn write_package(manifest: serde_json::Value) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("steps")).unwrap();
    std::fs::write(
        directory.path().join("steps/one.json"),
        serde_json::to_vec(&serde_json::json!({
            "step_id": "one",
            "title": "One",
            "actions": [{"type": "Present"}]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    directory
}

fn one_chapter(unit: serde_json::Value) -> serde_json::Value {
    unit
}

#[test]
fn a_missing_dir_name_falls_back_to_the_node_id() {
    let package = write_package(serde_json::json!({
        "id": "fallback",
        "title": "Fallback",
        "children": [one_chapter(serde_json::json!({
            "id": "chapter_1",
            "title": "Chapter 1",
            "children": [{
                "id": "hello",
                "title": "Hello",
                "steps": [{"step_file": "steps/one.json"}]
            }]
        }))]
    }));

    let course = load_package(package.path()).unwrap();

    assert_eq!(course.units[0].path, "chapter_1/hello");
    assert_eq!(course.units[0].dir_path, "chapter_1/hello");
    assert_eq!(
        course.node_by_path("chapter_1/hello").unwrap().dir_path(),
        "chapter_1/hello"
    );
}

#[test]
fn dir_name_changes_layout_but_never_the_node_path() {
    let package = write_package(serde_json::json!({
        "id": "layout",
        "title": "Layout",
        "children": [one_chapter(serde_json::json!({
            "id": "chapter_1",
            "title": "第一章",
            "dir_name": "chapter-one",
            "children": [{
                "id": "hello",
                "title": "你好",
                "dir_name": "hello-world",
                "steps": [{"step_file": "steps/one.json"}]
            }]
        }))]
    }));

    let course = load_package(package.path()).unwrap();

    // Identity stays on ids.
    assert_eq!(course.units[0].path, "chapter_1/hello");
    // Layout follows dir names.
    assert_eq!(course.units[0].dir_path, "chapter-one/hello-world");
    let node = course.node_by_path("chapter_1/hello").unwrap();
    assert_eq!(node.id(), "hello");
    assert_eq!(node.dir_name(), "hello-world");
    assert_eq!(node.dir_path(), "chapter-one/hello-world");
    assert_eq!(
        course.directory_path_for("chapter_1/hello"),
        Some("chapter-one/hello-world")
    );
}

#[test]
fn seeds_and_resources_resolve_through_the_directory_path() {
    let package = write_package(serde_json::json!({
        "id": "layout",
        "title": "Layout",
        "children": [one_chapter(serde_json::json!({
            "id": "chapter_1",
            "title": "第一章",
            "dir_name": "chapter-one",
            "children": [{
                "id": "hello",
                "title": "你好",
                "dir_name": "hello-world",
                "steps": [{"step_file": "steps/one.json"}]
            }]
        }))]
    }));
    let root = package.path();

    std::fs::create_dir_all(root.join("seeds/chapter-one/hello-world")).unwrap();
    std::fs::write(
        root.join("seeds/chapter-one/hello-world/main.rs"),
        "fn main() {}\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("resources/chapter-one/hello-world")).unwrap();

    let course = load_package(root).unwrap();

    assert_eq!(
        course.workspace_seed_paths_for_path("chapter_1/hello"),
        vec![root.join("seeds/chapter-one/hello-world")]
    );
    let resolved = course
        .resolve_resource_aliases_for_path("{{resource:node}}/input.txt", "chapter_1/hello")
        .unwrap();
    let resolved = resolved.replace('\\', "/");
    assert!(
        resolved.ends_with("resources/chapter-one/hello-world/input.txt"),
        "resource alias must use the Directory Path: {resolved}"
    );
    let ancestor = course
        .resolve_resource_aliases_for_path("{{resource:ancestor}}", "chapter_1/hello")
        .unwrap()
        .replace('\\', "/");
    assert!(
        ancestor.ends_with("resources/chapter-one"),
        "ancestor alias must use the Directory Path: {ancestor}"
    );
}

#[test]
fn duplicate_sibling_dir_names_are_rejected() {
    let package = write_package(serde_json::json!({
        "id": "collision",
        "title": "Collision",
        "children": [{
            "id": "a",
            "title": "A",
            "dir_name": "same",
            "steps": [{"step_file": "steps/one.json"}]
        }, {
            "id": "b",
            "title": "B",
            "dir_name": "same",
            "steps": [{"step_file": "steps/one.json"}]
        }]
    }));

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("dir_name `same` appears more than once"),
        "duplicate directory names must fail loudly: {error}"
    );
}

#[test]
fn sibling_dir_names_that_differ_only_by_case_are_rejected() {
    let package = write_package(serde_json::json!({
        "id": "dir_case_collision",
        "title": "Dir Case Collision",
        "children": [{
            "id": "a",
            "title": "A",
            "dir_name": "Same",
            "steps": [{"step_file": "steps/one.json"}]
        }, {
            "id": "b",
            "title": "B",
            "dir_name": "same",
            "steps": [{"step_file": "steps/one.json"}]
        }]
    }));

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("dir_name `same` appears more than once"),
        "case-only directory name collisions must fail loudly: {error}"
    );
}

#[test]
fn unsafe_dir_names_are_rejected() {
    let package = write_package(serde_json::json!({
        "id": "unsafe_dir",
        "title": "Unsafe Dir",
        "children": [{
            "id": "unit",
            "title": "Unit",
            "dir_name": "bad/name",
            "steps": [{"step_file": "steps/one.json"}]
        }]
    }));

    let error = load_package(package.path()).unwrap_err().to_string();

    assert!(
        error.contains("dir_name `bad/name` must be a single safe path segment"),
        "dir_name must obey the same path-segment rule as id: {error}"
    );
}
