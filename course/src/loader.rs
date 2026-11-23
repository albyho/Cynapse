use super::*;

use super::model::{InheritedNodeDefaults, merge_assets, resolve_inherited};
use super::steps::load_step;
use super::validation::{validate_id_path_segment, validate_requirements, validate_seed_scopes};

/// Load a package and reject every package validity error.
///
/// Errors short-circuit; use [`check_package`] when every semantic finding is
/// needed.
pub fn load_package(root: &Path) -> Result<Course, CourseError> {
    check::load_package_with_report(root).into_result()
}

pub(super) fn load_package_unchecked(root: &Path) -> Result<Course, CourseError> {
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(root.join("manifest.json"))?)?;
    load_package_from_manifest(root, manifest)
}

fn load_package_from_manifest(root: &Path, manifest: Manifest) -> Result<Course, CourseError> {
    let tree = build_course_root(&manifest)?;

    let canonical_root = root.canonicalize()?;
    validate_course_workspace(
        root,
        &canonical_root,
        manifest
            .workspace
            .as_deref()
            .unwrap_or(super::DEFAULT_WORKSPACE),
    )?;
    let units = collect_units(&tree);
    validate_seed_scopes(root, &canonical_root, &units)?;
    let mut steps = Vec::new();
    let mut audio_cache = HashMap::new();

    for entry in &units {
        let unit_audio_assets = load_unit_audio_assets(
            root,
            &canonical_root,
            &entry.path,
            &entry.unit.assets,
            &mut audio_cache,
        )?;
        let mut unit_step_ids = HashSet::new();
        for reference in &entry.unit.steps {
            let (step_file, step) =
                load_step(root, &canonical_root, reference, &unit_audio_assets)?;
            if !unit_step_ids.insert(step.step_id.clone()) {
                return Err(CourseError::Package(format!(
                    "step_id `{}` appears more than once in unit `{}`",
                    step.step_id, entry.path
                )));
            }
            steps.push(LoadedStep {
                parent_id: entry.parent_id.clone(),
                parent_title: entry.parent_title.clone(),
                unit_title: entry.unit.title.clone(),
                unit_path: entry.path.clone(),
                step_file,
                step,
            });
        }
    }

    Ok(Course {
        manifest,
        tree,
        units,
        steps,
        package_root: root.to_path_buf(),
    })
}

const RESERVED_WORKSPACE_DIRS: [&str; 4] = ["steps", "seeds", "resources", "assets"];

fn is_reserved_workspace_dir(name: &str) -> bool {
    RESERVED_WORKSPACE_DIRS
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
}

/// A symlink or junction can point the workspace at a package-owned directory,
/// so the literal first segment is not enough once the path exists.
pub(super) fn resolves_into_reserved_dir(canonical_root: &Path, canonical_path: &Path) -> bool {
    canonical_path
        .strip_prefix(canonical_root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|first| match first {
            std::path::Component::Normal(name) => {
                is_reserved_workspace_dir(&name.to_string_lossy())
            }
            _ => false,
        })
}

fn validate_course_workspace(
    root: &Path,
    canonical_root: &Path,
    workspace: &str,
) -> Result<(), CourseError> {
    if workspace.trim().is_empty() {
        return Err(CourseError::Package(
            "workspace path must not be empty".to_string(),
        ));
    }
    if workspace.contains('\\') {
        return Err(CourseError::Package(format!(
            "workspace `{workspace}` must use forward slashes"
        )));
    }
    let relative = Path::new(workspace);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CourseError::Package(format!(
            "workspace `{workspace}` is outside the Course Package"
        )));
    }
    let mut segments = workspace.split('/');
    let first_segment = segments.next().unwrap_or_default();
    for segment in std::iter::once(first_segment).chain(segments) {
        validate_id_path_segment("workspace path segment", segment)?;
    }
    if first_segment != super::DEFAULT_WORKSPACE {
        return Err(CourseError::Package(format!(
            "workspace `{workspace}` must stay inside the Course Package `{}/` directory",
            super::DEFAULT_WORKSPACE
        )));
    }
    let workspace_path = root.join(relative);
    if workspace_path.exists() {
        let canonical_workspace = workspace_path.canonicalize()?;
        if canonical_workspace == canonical_root {
            return Err(CourseError::Package(format!(
                "workspace `{workspace}` must not be the Course Package root"
            )));
        }
        if resolves_into_reserved_dir(canonical_root, &canonical_workspace) {
            return Err(CourseError::Package(format!(
                "workspace `{workspace}` resolves into a reserved Course Package directory"
            )));
        }
        if !canonical_workspace.starts_with(canonical_root) || !canonical_workspace.is_dir() {
            return Err(CourseError::Package(format!(
                "workspace `{workspace}` must reference a directory inside the Course Package"
            )));
        }
    } else {
        let existing_parent = workspace_path
            .parent()
            .and_then(nearest_existing_path)
            .ok_or_else(|| {
                CourseError::Package(format!(
                    "workspace `{workspace}` has no existing parent inside the Course Package"
                ))
            })?;
        let canonical_parent = existing_parent.canonicalize()?;
        if !canonical_parent.starts_with(canonical_root) {
            return Err(CourseError::Package(format!(
                "workspace `{workspace}` is outside the Course Package"
            )));
        }
        if resolves_into_reserved_dir(canonical_root, &canonical_parent) {
            return Err(CourseError::Package(format!(
                "workspace `{workspace}` resolves into a reserved Course Package directory"
            )));
        }
    }
    Ok(())
}

fn nearest_existing_path(mut path: &Path) -> Option<&Path> {
    while !path.exists() {
        path = path.parent()?;
    }
    Some(path)
}

pub(super) fn build_course_root(manifest: &Manifest) -> Result<CourseNode, CourseError> {
    validate_id_path_segment("id", &manifest.id)?;
    if manifest.children.is_empty() {
        return Err(CourseError::Package(
            "course root must contain at least one node".to_string(),
        ));
    }

    let inherited = InheritedNodeDefaults {
        adapters: manifest.adapters.resolve(&[]),
        environment: manifest.environment.resolve(&[]),
        assets: merge_assets(&[], Some(&manifest.assets))?,
    };
    validate_requirements(&inherited.adapters, &inherited.environment, "course root")?;
    let mut sibling_ids = HashSet::new();
    let mut sibling_dir_names = HashSet::new();
    let children = manifest
        .children
        .iter()
        .map(|child| {
            if child.id.eq_ignore_ascii_case("resources") {
                return Err(CourseError::Package(format!(
                    "root node id `{}` is reserved: its Unit Workspace would collide with `workspace/resources`",
                    child.id
                )));
            }
            build_tree_node(
                child,
                "",
                "",
                &mut sibling_ids,
                &mut sibling_dir_names,
                &inherited,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CourseNode {
        id: manifest.id.clone(),
        dir_name: String::new(),
        dir_path: String::new(),
        title: manifest.title.clone(),
        adapters: inherited.adapters,
        environment: inherited.environment,
        assets: inherited.assets,
        steps: Vec::new(),
        children,
    })
}

fn build_tree_node(
    document: &CourseNodeDocument,
    parent_path: &str,
    parent_dir_path: &str,
    sibling_ids: &mut HashSet<String>,
    sibling_dir_names: &mut HashSet<String>,
    inherited: &InheritedNodeDefaults,
) -> Result<CourseNode, CourseError> {
    validate_id_path_segment("id", &document.id)?;
    let dir_name = document
        .dir_name
        .clone()
        .unwrap_or_else(|| document.id.clone());
    validate_id_path_segment("dir_name", &dir_name)?;
    if !sibling_ids.insert(document.id.to_lowercase()) {
        let parent = if parent_path.is_empty() {
            "course root"
        } else {
            parent_path
        };
        return Err(CourseError::Package(format!(
            "node id `{}` appears more than once under `{parent}` (sibling ids are unique case-insensitively)",
            document.id
        )));
    }
    if !sibling_dir_names.insert(dir_name.to_lowercase()) {
        let parent = if parent_path.is_empty() {
            "course root"
        } else {
            parent_path
        };
        return Err(CourseError::Package(format!(
            "node dir_name `{dir_name}` appears more than once under `{parent}` (sibling dir_names are unique case-insensitively)"
        )));
    }
    let path = if parent_path.is_empty() {
        document.id.clone()
    } else {
        format!("{parent_path}/{}", document.id)
    };
    let dir_path = if parent_dir_path.is_empty() {
        dir_name.clone()
    } else {
        format!("{parent_dir_path}/{dir_name}")
    };

    let effective = InheritedNodeDefaults {
        adapters: resolve_inherited(document.adapters.as_ref(), &inherited.adapters),
        environment: resolve_inherited(document.environment.as_ref(), &inherited.environment),
        assets: merge_assets(&inherited.assets, document.assets.as_ref())?,
    };
    validate_requirements(
        &effective.adapters,
        &effective.environment,
        &format!("node `{path}`"),
    )?;

    let steps = document.steps.clone().unwrap_or_default();
    let mut child_ids = HashSet::new();
    let mut child_dir_names = HashSet::new();
    let children = document
        .children
        .as_ref()
        .map(|children| {
            children
                .iter()
                .map(|child| {
                    build_tree_node(
                        child,
                        &path,
                        &dir_path,
                        &mut child_ids,
                        &mut child_dir_names,
                        &effective,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    if steps.is_empty() && children.is_empty() {
        return Err(CourseError::Package(format!(
            "node `{path}` declares neither steps nor children"
        )));
    }

    Ok(CourseNode {
        id: document.id.clone(),
        dir_name,
        dir_path,
        title: document.title.clone(),
        adapters: effective.adapters,
        environment: effective.environment,
        assets: effective.assets,
        steps,
        children,
    })
}

pub(super) fn collect_units(root: &CourseNode) -> Vec<UnitEntry> {
    fn walk(node: &CourseNode, parent_path: &str, units: &mut Vec<UnitEntry>) {
        for child in &node.children {
            let path = if parent_path.is_empty() {
                child.id.clone()
            } else {
                format!("{parent_path}/{}", child.id)
            };
            if child.has_steps() {
                units.push(UnitEntry {
                    path: path.clone(),
                    dir_path: child.dir_path.clone(),
                    parent_id: node.id.clone(),
                    parent_title: node.title.clone(),
                    unit: Unit {
                        unit_id: child.id.clone(),
                        title: child.title.clone(),
                        assets: child.assets.clone(),
                        adapters: child.adapters.clone(),
                        environment: child.environment.clone(),
                        steps: child.steps.clone(),
                    },
                });
            }
            walk(child, &path, units);
        }
    }

    let mut units = Vec::new();
    walk(root, "", &mut units);
    units
}
