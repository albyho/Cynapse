use super::*;

fn facts(target: TargetState, parent: TargetState, directory_empty: bool) -> TargetFacts {
    TargetFacts {
        target,
        parent,
        directory_empty,
    }
}

#[test]
fn create_file_rejects_a_directory_target() {
    let action = Action::CreateFile(crate::CreateFileAction {
        path: "demo.txt".into(),
        create_parents: true,
    });
    assert_eq!(
        plan_file_action(
            &action,
            facts(TargetState::Directory, TargetState::Directory, true)
        ),
        Err(FileActionError::TargetIsDirectory)
    );
}

#[test]
fn create_rejects_a_file_as_parent_even_when_creating_parents() {
    let action = Action::CreateFile(crate::CreateFileAction {
        path: "demo.txt/nested.txt".into(),
        create_parents: true,
    });
    assert_eq!(
        plan_file_action(
            &action,
            facts(TargetState::Missing, TargetState::File, true)
        ),
        Err(FileActionError::ParentNotDirectory)
    );
}

#[test]
fn create_without_parents_requires_the_parent_to_exist() {
    let action = Action::CreateFile(crate::CreateFileAction {
        path: "nested/demo.txt".into(),
        create_parents: false,
    });
    assert_eq!(
        plan_file_action(
            &action,
            facts(TargetState::Missing, TargetState::Missing, true)
        ),
        Err(FileActionError::ParentMissing)
    );
}

#[test]
fn delete_file_respects_if_missing() {
    let strict = Action::DeleteFile(crate::DeleteFileAction {
        path: "demo.txt".into(),
        if_missing: crate::MissingTargetPolicy::Error,
    });
    assert_eq!(
        plan_file_action(
            &strict,
            facts(TargetState::Missing, TargetState::Directory, true)
        ),
        Err(FileActionError::TargetMissing)
    );

    let ignored = Action::DeleteFile(crate::DeleteFileAction {
        path: "demo.txt".into(),
        if_missing: crate::MissingTargetPolicy::Ignore,
    });
    assert_eq!(
        plan_file_action(
            &ignored,
            facts(TargetState::Missing, TargetState::Directory, true)
        ),
        Ok(FileActionPlan::SkipMissing)
    );
}

#[test]
fn delete_directory_requires_recursive_for_a_non_empty_target() {
    let action = Action::DeleteDirectory(crate::DeleteDirectoryAction {
        path: "demo".into(),
        recursive: false,
        if_missing: crate::MissingTargetPolicy::Error,
    });
    assert_eq!(
        plan_file_action(
            &action,
            facts(TargetState::Directory, TargetState::Directory, false)
        ),
        Err(FileActionError::DirectoryNotEmpty)
    );
}

#[test]
fn shared_cases_match_the_adapter_contract() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/contracts/file-action-cases.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let action_value = case["action"].clone();
        let path = action_value["path"].as_str().unwrap().to_string();
        let action: Action = serde_json::from_value(action_value).unwrap();
        let fs = case["fs"].as_object().cloned().unwrap_or_default();
        let state = |candidate: &str| match fs.get(candidate).and_then(|value| value.as_str()) {
            Some("file") => TargetState::File,
            Some("directory") => TargetState::Directory,
            _ => TargetState::Missing,
        };
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| state(parent))
            .unwrap_or(TargetState::Directory);
        let directory_empty = state(&path) != TargetState::Directory
            || !fs.keys().any(|key| key.starts_with(&format!("{path}/")));
        let outcome = plan_file_action(
            &action,
            TargetFacts {
                target: state(&path),
                parent,
                directory_empty,
            },
        );
        let expected = &case["expected"];
        if expected["status"] == "ok" {
            assert!(outcome.is_ok(), "{name}: expected ok, got {outcome:?}");
        } else {
            let expected_code = expected["code"].as_str().unwrap();
            let actual = match outcome.unwrap_err() {
                FileActionError::TargetIsDirectory | FileActionError::TargetIsFile => {
                    "target_type_conflict"
                }
                FileActionError::TargetMissing => "target_missing",
                FileActionError::ParentMissing | FileActionError::ParentNotDirectory => {
                    "parent_missing"
                }
                FileActionError::DirectoryNotEmpty => "directory_not_empty",
                FileActionError::NotAFileAction => "not_a_file_action",
            };
            assert_eq!(actual, expected_code, "{name}");
        }
    }
}
