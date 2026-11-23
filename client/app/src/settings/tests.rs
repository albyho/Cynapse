use super::*;

#[test]
fn settings_path_keeps_the_tauri_app_data_directory() {
    let path = settings_path_from(Some(OsStr::new("app-data"))).expect("APPDATA is present");
    assert_eq!(
        path,
        PathBuf::from("app-data")
            .join("com.cynapse.client")
            .join("settings.json")
    );
}

#[test]
fn corrupt_settings_use_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, "not json").unwrap();

    assert_eq!(load_from(&path), ClientSettings::default());
}

#[test]
fn language_preference_round_trips_through_the_settings_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let settings = ClientSettings {
        language: LanguagePreference::ZhCn,
        ..ClientSettings::default()
    };

    save_to(&path, &settings).unwrap();
    assert_eq!(load_from(&path).language, LanguagePreference::ZhCn);
}

#[test]
fn course_store_dir_round_trips_through_the_settings_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let course_store_dir = directory.path().join("store");
    let settings = ClientSettings {
        course_store_dir: Some(course_store_dir.clone()),
        ..ClientSettings::default()
    };

    save_to(&path, &settings).unwrap();

    assert_eq!(load_from(&path).course_store_dir, Some(course_store_dir));
}

#[test]
fn settings_without_a_version_use_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();

    assert_eq!(load_from(&path), ClientSettings::default());
}

#[test]
fn settings_with_a_different_version_use_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, r#"{"version":0,"theme":"dark"}"#).unwrap();

    assert_eq!(load_from(&path), ClientSettings::default());
}

#[test]
fn volume_above_full_scale_is_clamped_on_load() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(
        &path,
        format!(r#"{{"version":{CLIENT_SETTINGS_VERSION},"volume":200}}"#),
    )
    .unwrap();

    assert_eq!(load_from(&path).volume, 100);
}
