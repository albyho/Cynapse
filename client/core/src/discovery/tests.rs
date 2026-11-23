use super::*;

#[test]
fn discovery_file_round_trips() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.json");
    let workspace = "test-workspace";
    let discovery = new_discovery(43210, workspace);
    let handle = DiscoveryHandle::write(path, &discovery).unwrap();

    let contents = std::fs::read_to_string(handle.path()).unwrap();
    let parsed: DiscoveryFile = serde_json::from_str(&contents).unwrap();

    assert_eq!(parsed, discovery);
    assert!(!parsed.token.is_empty());
    assert_eq!(parsed.pid, std::process::id());
    assert_eq!(parsed.workspace, workspace);
}

#[test]
fn discovery_file_replaces_an_existing_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.json");
    let first = new_discovery(43210, "first-workspace");
    DiscoveryHandle::write(&path, &first).unwrap();

    let replacement = new_discovery(43211, "second-workspace");
    DiscoveryHandle::write(&path, &replacement).unwrap();

    let parsed: DiscoveryFile = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(parsed, replacement);
}

#[test]
fn concurrent_writers_get_private_temporary_paths() {
    let path = global_runtime_path();
    let first = temporary_discovery_path(&path);
    let second = temporary_discovery_path(&path);

    assert_ne!(first, second);
    assert_ne!(first, path.with_extension("json.tmp"));
    assert_eq!(first.parent(), path.parent());
    assert!(
        first
            .to_string_lossy()
            .contains(&std::process::id().to_string())
    );
}

#[test]
fn concurrent_writers_leave_a_complete_file_and_no_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.json");
    let baseline = new_discovery(43210, "baseline");
    DiscoveryHandle::write(&path, &baseline).unwrap();

    let candidates = [
        new_discovery(43211, "first-writer"),
        new_discovery(43212, "second-writer"),
    ];
    let writers: Vec<_> = candidates
        .iter()
        .map(|discovery| {
            let path = path.clone();
            let discovery = discovery.clone();
            std::thread::spawn(move || DiscoveryHandle::write(&path, &discovery))
        })
        .collect();
    for writer in writers {
        let _ = writer.join();
    }

    let contents = std::fs::read(&path).unwrap();
    let parsed: DiscoveryFile = serde_json::from_slice(&contents).unwrap();
    assert!(parsed == baseline || candidates.contains(&parsed));
    assert_eq!(
        std::fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count(),
        0
    );
}

#[test]
fn discovery_handle_removes_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.json");
    let handle = DiscoveryHandle::write(path, &new_discovery(43210, "workspace")).unwrap();

    handle.remove().unwrap();

    assert!(!handle.path().exists());
    handle.remove().unwrap();
}

#[test]
fn an_old_handle_does_not_remove_a_newer_discovery_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.json");
    let old = DiscoveryHandle::write(&path, &new_discovery(43210, "old")).unwrap();
    let newer = new_discovery(43211, "new");
    DiscoveryHandle::write(&path, &newer).unwrap();

    old.remove().unwrap();

    let parsed: DiscoveryFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(parsed, newer);
}

#[test]
fn a_corrupt_discovery_file_is_left_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.json");
    let handle = DiscoveryHandle::write(&path, &new_discovery(43210, "workspace")).unwrap();
    std::fs::write(&path, "{not json").unwrap();

    handle.remove().unwrap();

    assert!(path.exists());
}

#[test]
fn global_runtime_path_is_a_single_file() {
    let path = global_runtime_path();

    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("runtime.json")
    );
    assert_eq!(
        path.parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str()),
        Some("Client")
    );
    assert_eq!(
        path.parent()
            .and_then(|parent| parent.parent())
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str()),
        Some("Cynapse")
    );
}

#[test]
fn runtime_dir_prefers_platform_data_then_home() {
    let local = Path::new("local-data");
    let app = Path::new("app-data");
    let home = Path::new("home-data");

    assert_eq!(
        runtime_dir_from(
            Some(local.as_os_str()),
            Some(app.as_os_str()),
            Some(home.as_os_str())
        ),
        local.join("Cynapse").join("Client")
    );
    assert_eq!(
        runtime_dir_from(None, Some(app.as_os_str()), Some(home.as_os_str())),
        app.join("Cynapse").join("Client")
    );
    assert_eq!(
        runtime_dir_from(None, None, Some(home.as_os_str())),
        home.join("Cynapse").join("Client")
    );
    assert_eq!(
        runtime_dir_from(None, None, None),
        PathBuf::from(".").join("Cynapse").join("Client")
    );
}

#[cfg(unix)]
#[test]
fn discovery_files_are_private_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.json");
    DiscoveryHandle::write(&path, &new_discovery(43210, "workspace")).unwrap();

    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(directory.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn global_state_db_path_lives_beside_runtime_discovery() {
    let path = global_state_db_path();

    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("state.db")
    );
    assert_eq!(path.parent(), global_runtime_path().parent());
}
