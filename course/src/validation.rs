use super::*;

/// Seed 目录按 Unit Path 约定推导为 `seeds/<unit path>/`，因此路径本身不来自
/// 课程包、也就不可能越界。这里只校验实际存在的 Seed 目录没有用符号链接
/// 逃出课程包，并且确实是目录。
pub(super) fn validate_seed_scopes(
    root: &Path,
    canonical_root: &Path,
    units: &[UnitEntry],
) -> Result<(), CourseError> {
    let seeds = root.join("seeds");
    if !seeds.is_dir() {
        return Ok(());
    }
    let canonical_seeds = seeds.canonicalize()?;
    if !canonical_seeds.starts_with(canonical_root) {
        return Err(CourseError::Package(
            "the `seeds` directory must resolve inside the Course Package".into(),
        ));
    }
    for unit in units {
        let directory = seeds.join(&unit.dir_path);
        if !directory.exists() {
            continue;
        }
        let canonical = directory.canonicalize()?;
        if !canonical.starts_with(&canonical_seeds) || !canonical.is_dir() {
            return Err(CourseError::Package(format!(
                "seed for unit `{}` must be a directory inside `seeds/`",
                unit.path
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_requirements(
    adapters: &[AdapterId],
    requirements: &[EnvironmentRequirement],
    location: &str,
) -> Result<(), CourseError> {
    if adapters.len() > 1 {
        return Err(CourseError::Package(format!(
            "{location} requires multiple adapters, but this build supports one adapter per node"
        )));
    }
    let mut adapter_ids = HashSet::new();
    for adapter in adapters {
        if !adapter_ids.insert(*adapter) {
            return Err(CourseError::Package(format!(
                "{location} has duplicate adapter requirement `{}`",
                adapter.connection_name()
            )));
        }
    }

    let mut environment_ids = HashSet::new();
    for requirement in requirements {
        if requirement.id.trim().is_empty() || !environment_ids.insert(requirement.id.clone()) {
            return Err(CourseError::Package(format!(
                "{location} has an empty or duplicate environment requirement id `{}`",
                requirement.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_id_path_segment(field: &str, value: &str) -> Result<(), CourseError> {
    const RESERVED_WINDOWS_NAMES: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let invalid_character = value.chars().any(|character| {
        character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
    });
    let stem = value.split_once('.').map_or(value, |(stem, _)| stem);
    let reserved = RESERVED_WINDOWS_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved));
    if value.trim() != value
        || value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains('/')
        || value.contains('\\')
        || invalid_character
        || reserved
    {
        return Err(CourseError::Package(format!(
            "{field} `{value}` must be a single safe path segment"
        )));
    }
    Ok(())
}
