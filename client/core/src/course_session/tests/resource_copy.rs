use super::super::*;

fn assert_not_exists(path: &Path) {
    assert!(
        std::fs::symlink_metadata(path).is_err(),
        "{} should not exist",
        path.display()
    );
}

#[test]
fn successful_resource_copy_replaces_the_old_tree() {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(package.path().join("resources")).unwrap();
    std::fs::write(package.path().join("resources/new.txt"), "new").unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("resources")).unwrap();
    std::fs::write(workspace.path().join("resources/old.txt"), "old").unwrap();

    prepare_course_resources(package.path(), workspace.path()).unwrap();

    assert_eq!(
        std::fs::read_to_string(workspace.path().join("resources/new.txt")).unwrap(),
        "new"
    );
    assert_not_exists(&workspace.path().join("resources/old.txt"));
    assert_not_exists(&workspace.path().join(RESOURCE_TEMP_DIR));
}

#[test]
fn failed_resource_copy_preserves_the_old_tree_and_cleans_temp() {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(package.path().join("resources")).unwrap();
    std::fs::write(package.path().join("resources/new.txt"), "new").unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("resources")).unwrap();
    std::fs::write(workspace.path().join("resources/old.txt"), "old").unwrap();

    let error = replace_course_resources(package.path(), workspace.path(), |_, temp| {
        std::fs::write(temp.join("partial.txt"), "partial")?;
        Err(CourseSessionError::WorkspaceReset("copy failed".into()))
    })
    .unwrap_err();

    assert!(error.to_string().contains("copy failed"));
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("resources/old.txt")).unwrap(),
        "old"
    );
    assert_not_exists(&workspace.path().join(RESOURCE_TEMP_DIR));
}

#[test]
fn ensure_repairs_a_file_at_the_resources_path() {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(package.path().join("resources")).unwrap();
    std::fs::write(package.path().join("resources/data.txt"), "data").unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("resources"), "not a directory").unwrap();

    ensure_course_resources(package.path(), workspace.path()).unwrap();

    assert_eq!(
        std::fs::read_to_string(workspace.path().join("resources/data.txt")).unwrap(),
        "data"
    );
}

#[test]
fn ensure_removes_a_stale_resource_temp_directory() {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(package.path().join("resources")).unwrap();
    std::fs::write(package.path().join("resources/data.txt"), "data").unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("resources")).unwrap();
    std::fs::write(workspace.path().join("resources/old.txt"), "old").unwrap();
    std::fs::create_dir_all(workspace.path().join(RESOURCE_TEMP_DIR)).unwrap();
    std::fs::write(
        workspace.path().join(RESOURCE_TEMP_DIR).join("stale.txt"),
        "stale",
    )
    .unwrap();

    ensure_course_resources(package.path(), workspace.path()).unwrap();

    assert_not_exists(&workspace.path().join(RESOURCE_TEMP_DIR));
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("resources/old.txt")).unwrap(),
        "old"
    );
}
