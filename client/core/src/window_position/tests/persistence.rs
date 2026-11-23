use super::*;

#[test]
fn missing_file_loads_as_no_geometry() {
    let directory = tempfile::tempdir().unwrap();
    let store = WindowGeometryStore::new(directory.path().join("window-position.json"));

    assert_eq!(store.load().unwrap(), None);
}

#[test]
fn saved_geometry_can_be_loaded() {
    let directory = tempfile::tempdir().unwrap();
    let store = WindowGeometryStore::new(directory.path().join("window-position.json"));

    store.save(WindowGeometry::new(120, 45, 640, 720)).unwrap();

    assert_eq!(
        store.load().unwrap(),
        Some(WindowGeometry::new(120, 45, 640, 720))
    );
}

#[test]
fn a_position_only_file_still_loads_without_a_size() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("window-position.json");
    std::fs::write(&path, r#"{"x":120,"y":45}"#).unwrap();
    let store = WindowGeometryStore::new(path);

    assert_eq!(
        store.load().unwrap(),
        Some(WindowGeometry {
            x: 120,
            y: 45,
            width: None,
            height: None,
        })
    );
}

#[test]
fn corrupt_position_file_is_reported() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("window-position.json");
    std::fs::write(&path, "not json").unwrap();
    let store = WindowGeometryStore::new(path);

    assert!(store.load().is_err());
}
