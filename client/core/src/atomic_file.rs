//! Atomic replacement for small JSON state files.
//!
//! Each save writes a private temporary file, fsyncs it, then renames the
//! temporary file over the destination.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[cfg(test)]
thread_local! {
    static FAIL_BEFORE_REPLACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn fail_before_replace_once() {
    FAIL_BEFORE_REPLACE.with(|flag| flag.set(true));
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    path.with_file_name(format!(
        "{name}.{}.{}.tmp",
        std::process::id(),
        Uuid::new_v4()
    ))
}

pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);

        #[cfg(test)]
        let injected_failure = FAIL_BEFORE_REPLACE.with(|flag| flag.replace(false));
        #[cfg(not(test))]
        let injected_failure = false;

        if injected_failure {
            return Err(io::Error::other("injected atomic write failure"));
        }
        fs::rename(&temporary, path).map_err(|error| {
            io::Error::new(error.kind(), format!("could not replace target: {error}"))
        })?;

        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }

        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_replacement_keeps_the_previous_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        std::fs::write(&path, "old").unwrap();

        fail_before_replace_once();
        let error = write_atomic(&path, b"new").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }
}
