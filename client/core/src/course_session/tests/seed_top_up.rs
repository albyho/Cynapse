use super::super::*;
use std::fs;

#[test]
fn tops_up_missing_seed_files_without_overwriting() {
    let seed = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();

    fs::write(seed.path().join("keep.txt"), "seed version").unwrap();
    fs::write(seed.path().join("shared.txt"), "seed version").unwrap();
    fs::create_dir_all(seed.path().join("nested")).unwrap();
    fs::write(seed.path().join("nested/added.txt"), "added").unwrap();
    fs::create_dir_all(seed.path().join("child")).unwrap();
    fs::write(seed.path().join("child/child.txt"), "child").unwrap();

    // 学员已经改过的文件不应被 Seed 覆盖。
    fs::write(workspace.path().join("keep.txt"), "user edit").unwrap();

    let exclusions: HashSet<PathBuf> = [seed.path().join("child")].into_iter().collect();
    top_up_missing_seed_files(&[seed.path().to_path_buf()], workspace.path(), &exclusions).unwrap();

    assert_eq!(
        fs::read_to_string(workspace.path().join("keep.txt")).unwrap(),
        "user edit"
    );
    assert_eq!(
        fs::read_to_string(workspace.path().join("shared.txt")).unwrap(),
        "seed version"
    );
    assert_eq!(
        fs::read_to_string(workspace.path().join("nested/added.txt")).unwrap(),
        "added"
    );
    // 后代节点的 Seed 目录整棵跳过。
    assert!(!workspace.path().join("child").exists());
}

#[test]
fn copying_a_seed_skips_descendant_seed_directories() {
    let seed = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();

    fs::write(seed.path().join("own.txt"), "own").unwrap();
    fs::create_dir_all(seed.path().join("child/nested")).unwrap();
    fs::write(seed.path().join("child/child.txt"), "child").unwrap();
    fs::write(seed.path().join("child/nested/deep.txt"), "deep").unwrap();
    fs::create_dir_all(seed.path().join("assets")).unwrap();
    fs::write(seed.path().join("assets/data.txt"), "data").unwrap();

    let exclusions: HashSet<PathBuf> = [seed.path().join("child")].into_iter().collect();
    let destination = workspace.path().join("unit");
    copy_directory_overlay(seed.path(), &destination, &exclusions).unwrap();

    assert_eq!(
        fs::read_to_string(destination.join("own.txt")).unwrap(),
        "own"
    );
    assert_eq!(
        fs::read_to_string(destination.join("assets/data.txt")).unwrap(),
        "data"
    );
    assert!(!destination.join("child").exists());
}
