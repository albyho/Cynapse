use crate::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    Missing,
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetFacts {
    pub target: TargetState,
    pub parent: TargetState,
    pub directory_empty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileActionPlan {
    CreateFile { create_parents: bool },
    CreateDirectory { create_parents: bool },
    DeleteFile,
    DeleteDirectory { recursive: bool },
    SkipMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileActionError {
    TargetIsDirectory,
    TargetIsFile,
    TargetMissing,
    ParentMissing,
    ParentNotDirectory,
    DirectoryNotEmpty,
    NotAFileAction,
}

/// Shared policy for the four filesystem Actions. Consumers supply the target
/// facts and perform only the I/O their environment needs.
pub fn plan_file_action(
    action: &Action,
    facts: TargetFacts,
) -> Result<FileActionPlan, FileActionError> {
    match action {
        Action::CreateFile(action) => {
            if facts.target == TargetState::Directory {
                return Err(FileActionError::TargetIsDirectory);
            }
            if facts.parent == TargetState::File {
                return Err(FileActionError::ParentNotDirectory);
            }
            if !action.create_parents && facts.parent != TargetState::Directory {
                return Err(FileActionError::ParentMissing);
            }
            Ok(FileActionPlan::CreateFile {
                create_parents: action.create_parents,
            })
        }
        Action::CreateDirectory(action) => {
            if facts.target == TargetState::File {
                return Err(FileActionError::TargetIsFile);
            }
            if facts.parent == TargetState::File {
                return Err(FileActionError::ParentNotDirectory);
            }
            if !action.create_parents && facts.parent != TargetState::Directory {
                return Err(FileActionError::ParentMissing);
            }
            Ok(FileActionPlan::CreateDirectory {
                create_parents: action.create_parents,
            })
        }
        Action::DeleteFile(action) => match facts.target {
            TargetState::Directory => Err(FileActionError::TargetIsDirectory),
            TargetState::Missing
                if matches!(action.if_missing, crate::MissingTargetPolicy::Ignore) =>
            {
                Ok(FileActionPlan::SkipMissing)
            }
            TargetState::Missing => Err(FileActionError::TargetMissing),
            TargetState::File => Ok(FileActionPlan::DeleteFile),
        },
        Action::DeleteDirectory(action) => match facts.target {
            TargetState::File => Err(FileActionError::TargetIsFile),
            TargetState::Missing
                if matches!(action.if_missing, crate::MissingTargetPolicy::Ignore) =>
            {
                Ok(FileActionPlan::SkipMissing)
            }
            TargetState::Missing => Err(FileActionError::TargetMissing),
            TargetState::Directory => {
                if !action.recursive && !facts.directory_empty {
                    return Err(FileActionError::DirectoryNotEmpty);
                }
                Ok(FileActionPlan::DeleteDirectory {
                    recursive: action.recursive,
                })
            }
        },
        _ => Err(FileActionError::NotAFileAction),
    }
}
