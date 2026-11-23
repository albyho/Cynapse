use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentCheckItem {
    pub id: String,
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentCheckReport {
    pub ok: bool,
    pub checks: Vec<EnvironmentCheckItem>,
}

pub fn output_satisfies_minimum_version(output: &str, minimum: &str) -> bool {
    let Some(actual) = find_version(output) else {
        return false;
    };
    let Some(required) = parse_version(minimum) else {
        return false;
    };
    actual >= required
}

fn find_version(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|token| {
        parse_version(token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && character != '.' && character != '-'
        }))
    })
}

fn parse_version(value: &str) -> Option<Version> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .split('-')
        .next()?
        .parse()
        .ok()?;
    Some(Version {
        major,
        minor,
        patch,
    })
}

#[cfg(test)]
mod tests;
