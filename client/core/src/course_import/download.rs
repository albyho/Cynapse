use super::{CourseImportError, CourseImportProgress, CourseImportStage, download_error};
use reqwest::Url;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DOWNLOAD_CHUNK: usize = 64 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

pub(super) fn download(
    url: &Url,
    directory: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(CourseImportProgress),
) -> Result<PathBuf, CourseImportError> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(30))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(download_error)?;
    let mut response = client.get(url.clone()).send().map_err(download_error)?;
    if !response.status().is_success() {
        return Err(download_error(format_args!("HTTP {}", response.status())));
    }

    let total = response.content_length();
    let path = directory.join("course.zip");
    let mut file = File::create(&path).map_err(download_error)?;
    let mut downloaded = 0_u64;
    let mut last_report = Instant::now() - PROGRESS_INTERVAL;
    let mut last_percent = None;
    progress(progress_value(downloaded, total));

    let mut buffer = vec![0_u8; DOWNLOAD_CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CourseImportError::Cancelled);
        }
        let read = response.read(&mut buffer).map_err(download_error)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(download_error)?;
        downloaded += read as u64;
        let percent = percent(downloaded, total);
        let should_report = last_report.elapsed() >= PROGRESS_INTERVAL
            || percent != last_percent
            || total.is_some_and(|total| downloaded >= total);
        if should_report {
            last_report = Instant::now();
            last_percent = percent;
            progress(progress_value(downloaded, total));
        }
    }
    file.flush().map_err(download_error)?;

    if cancel.load(Ordering::Relaxed) {
        return Err(CourseImportError::Cancelled);
    }
    progress(progress_value(downloaded, total));
    Ok(path)
}

fn progress_value(downloaded: u64, total: Option<u64>) -> CourseImportProgress {
    CourseImportProgress {
        stage: CourseImportStage::Downloading,
        downloaded,
        total,
        percent: percent(downloaded, total),
    }
}

fn percent(downloaded: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|total| *total > 0)?;
    Some((downloaded.min(total).saturating_mul(100) / total) as u8)
}
