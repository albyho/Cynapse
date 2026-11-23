//! Import a Course Package from an HTTP(S) ZIP.
//!
//! The ZIP is downloaded to a temporary file, extracted into a staging
//! directory under the configured course store, validated with the normal
//! package loader, and installed with an atomic directory rename.

mod download;
mod extract;
#[cfg(test)]
mod tests;

use cynapse_client_i18n::{t, t_with};
use reqwest::Url;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CourseImportStage {
    Downloading,
    Extracting,
    Installing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseImportProgress {
    pub stage: CourseImportStage,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CourseImportError {
    InvalidUrl,
    Download(String),
    Extract(String),
    Manifest(String),
    Install(String),
    AlreadyExists(PathBuf),
    Cancelled,
}

impl fmt::Display for CourseImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => formatter.write_str(&t("course_import.error.invalid_url")),
            Self::Download(detail) => formatter.write_str(&t_with(
                "course_import.error.download",
                &[("detail", detail)],
            )),
            Self::Extract(detail) => formatter.write_str(&t_with(
                "course_import.error.extract",
                &[("detail", detail)],
            )),
            Self::Manifest(detail) => formatter.write_str(&t_with(
                "course_import.error.manifest",
                &[("detail", detail)],
            )),
            Self::Install(detail) => formatter.write_str(&t_with(
                "course_import.error.install",
                &[("detail", detail)],
            )),
            Self::AlreadyExists(path) => formatter.write_str(&t_with(
                "course_import.error.already_exists",
                &[("path", &path.display().to_string())],
            )),
            Self::Cancelled => formatter.write_str(&t("course_import.error.cancelled")),
        }
    }
}

impl std::error::Error for CourseImportError {}

/// Validate a user-entered course ZIP URL. Both HTTP and HTTPS are allowed.
pub fn validate_url(raw: &str) -> Result<Url, CourseImportError> {
    let url = Url::parse(raw.trim()).map_err(|_| CourseImportError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CourseImportError::InvalidUrl);
    }
    Ok(url)
}

/// The directory this URL would create, when the URL itself names the ZIP.
pub fn suggested_target(
    store_dir: &Path,
    raw_url: &str,
) -> Result<Option<PathBuf>, CourseImportError> {
    let url = validate_url(raw_url)?;
    Ok(directory_name(&url).map(|name| store_dir.join(name)))
}

/// Download, extract, validate, and install a Course Package.
///
/// On success the returned path is the installed course root. Only the directory named by
/// `confirmed_target` may be replaced, after the new package passes validation.
pub fn import_course(
    raw_url: &str,
    store_dir: &Path,
    confirmed_target: Option<&Path>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(CourseImportProgress),
) -> Result<PathBuf, CourseImportError> {
    let url = validate_url(raw_url)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(CourseImportError::Cancelled);
    }
    fs::create_dir_all(store_dir).map_err(install_error)?;

    let download_dir = tempfile::Builder::new()
        .prefix("cynapse-course-")
        .tempdir()
        .map_err(install_error)?;
    let zip_path = download::download(&url, download_dir.path(), cancel, progress)?;

    progress(CourseImportProgress {
        stage: CourseImportStage::Extracting,
        downloaded: 0,
        total: None,
        percent: None,
    });
    let staging = tempfile::Builder::new()
        .prefix(".cynapse-import-")
        .tempdir_in(store_dir)
        .map_err(install_error)?;
    let extracted = extract::extract(&zip_path, staging.path(), cancel)?;
    let fallback_name = format!("{}-{}", extracted.id, extracted.version);
    let name = directory_name(&url).unwrap_or_else(|| sanitize_directory_name(&fallback_name));
    let final_path = store_dir.join(name);

    if cancel.load(Ordering::Relaxed) {
        return Err(CourseImportError::Cancelled);
    }

    progress(CourseImportProgress {
        stage: CourseImportStage::Installing,
        downloaded: 0,
        total: None,
        percent: None,
    });
    if final_path.exists() {
        if confirmed_target != Some(final_path.as_path()) {
            return Err(CourseImportError::AlreadyExists(final_path));
        }
        // Rename the old tree out of the way instead of deleting it in place.
        // If any descendant (for example a VS Code workspace) is locked, the
        // rename fails before the old course is damaged.
        let backup = store_dir.join(format!(".cynapse-replaced-{}", uuid::Uuid::new_v4()));
        fs::rename(&final_path, &backup).map_err(install_error)?;
        if let Err(error) = fs::rename(&extracted.root, &final_path) {
            let _ = fs::rename(&backup, &final_path);
            return Err(install_error(error));
        }
        if let Err(error) = remove_path(&backup) {
            log::warn!(
                target: "cynapse.client",
                "kept the replaced course directory at {} because it is still in use: {}",
                backup.display(),
                error
            );
        }
    } else {
        fs::rename(&extracted.root, &final_path).map_err(install_error)?;
    }

    Ok(final_path)
}

fn download_error(error: impl std::fmt::Display) -> CourseImportError {
    CourseImportError::Download(error.to_string())
}

fn extract_error(error: impl std::fmt::Display) -> CourseImportError {
    CourseImportError::Extract(error.to_string())
}

fn install_error(error: impl std::fmt::Display) -> CourseImportError {
    CourseImportError::Install(error.to_string())
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn directory_name(url: &Url) -> Option<String> {
    let segment = url.path_segments()?.next_back()?;
    let stem = segment
        .strip_suffix(".zip")
        .or_else(|| segment.strip_suffix(".ZIP"))?;
    let name = sanitize_directory_name(stem);
    (!name.is_empty()).then_some(name)
}

fn sanitize_directory_name(raw: &str) -> String {
    let mut name = String::with_capacity(raw.len());
    for character in raw.trim().chars() {
        if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        {
            name.push('_');
        } else {
            name.push(character);
        }
    }
    let name = name.trim().trim_end_matches(['.', ' ']).to_string();
    if name.is_empty() || name == "." || name == ".." {
        return String::new();
    }
    let upper = name.to_ascii_uppercase();
    let reserved = matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved { format!("_{name}") } else { name }
}
