use super::{CourseImportError, extract_error};
use cynapse_course::load_package;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use zip::ZipArchive;

const EXTRACT_CHUNK: usize = 64 * 1024;

pub(super) struct ExtractedCourse {
    pub root: PathBuf,
    pub id: String,
    pub version: String,
}

pub(super) fn extract(
    zip_path: &Path,
    staging: &Path,
    cancel: &AtomicBool,
) -> Result<ExtractedCourse, CourseImportError> {
    let file = File::open(zip_path).map_err(extract_error)?;
    let mut archive = ZipArchive::new(file).map_err(extract_error)?;

    for index in 0..archive.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CourseImportError::Cancelled);
        }
        let mut entry = archive.by_index(index).map_err(extract_error)?;
        if entry.encrypted() {
            return Err(CourseImportError::Extract(
                "encrypted ZIP entries are not supported".into(),
            ));
        }
        if entry.is_symlink() {
            return Err(CourseImportError::Extract(
                "ZIP symlinks are not supported".into(),
            ));
        }
        let Some(relative) = entry.enclosed_name() else {
            return Err(CourseImportError::Extract(
                "ZIP entry leaves the extraction directory".into(),
            ));
        };
        if relative.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        }) {
            return Err(CourseImportError::Extract(
                "ZIP entry leaves the extraction directory".into(),
            ));
        }

        let output = staging.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(extract_error)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(extract_error)?;
        }
        let mut output = File::create(&output).map_err(extract_error)?;
        let mut buffer = vec![0_u8; EXTRACT_CHUNK];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(CourseImportError::Cancelled);
            }
            let read = entry.read(&mut buffer).map_err(extract_error)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).map_err(extract_error)?;
        }
    }

    let root = course_root(staging)?;
    let course =
        load_package(&root).map_err(|error| CourseImportError::Manifest(error.to_string()))?;
    Ok(ExtractedCourse {
        root,
        id: course.manifest.id.clone(),
        version: course.manifest.version.clone(),
    })
}

fn course_root(staging: &Path) -> Result<PathBuf, CourseImportError> {
    if staging.join("manifest.json").is_file() {
        return Ok(staging.to_path_buf());
    }
    let mut directories = Vec::new();
    for entry in fs::read_dir(staging).map_err(extract_error)? {
        let entry = entry.map_err(extract_error)?;
        if entry.file_type().map_err(extract_error)?.is_dir() {
            directories.push(entry.path());
        }
    }
    if directories.len() == 1 && directories[0].join("manifest.json").is_file() {
        return Ok(directories.remove(0));
    }
    Err(CourseImportError::Manifest(
        "manifest.json must be at the ZIP root or in one top-level directory".into(),
    ))
}
