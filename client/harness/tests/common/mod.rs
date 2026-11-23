use cynapse_client_core::{CourseTreeNodeView, UnitView};
use std::path::PathBuf;

#[allow(dead_code)]
pub fn fixture(name: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shared = manifest.join("../../course/test-data").join(name);
    if shared.is_dir() {
        shared
    } else {
        manifest.join("../test-data").join(name)
    }
}

pub fn unit_by_path<'a>(nodes: &'a [CourseTreeNodeView], path: &str) -> &'a UnitView {
    fn find<'a>(nodes: &'a [CourseTreeNodeView], path: &str) -> Option<&'a UnitView> {
        for node in nodes {
            if let Some(unit) = node.unit.as_ref().filter(|unit| unit.path == path) {
                return Some(unit);
            }
            if let Some(unit) = find(&node.children, path) {
                return Some(unit);
            }
        }
        None
    }
    find(nodes, path).unwrap_or_else(|| panic!("unit {path} not found"))
}
