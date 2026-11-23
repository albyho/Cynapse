use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryFile {
    pub workspace: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub started_at: u64,
}

#[derive(Debug)]
pub enum DiscoveryError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "discovery I/O error: {error}"),
            Self::Json(error) => write!(formatter, "discovery JSON error: {error}"),
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for DiscoveryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DiscoveryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Per-product local data root. The Client and Cynapse Studio are separate
/// products and must never share a runtime directory or state database.
pub fn global_runtime_dir() -> PathBuf {
    let local_app_data = std::env::var_os("LOCALAPPDATA");
    let app_data = std::env::var_os("APPDATA");
    let home = std::env::var_os("HOME");
    runtime_dir_from(
        local_app_data.as_deref(),
        app_data.as_deref(),
        home.as_deref(),
    )
}

fn runtime_dir_from(
    local_app_data: Option<&OsStr>,
    app_data: Option<&OsStr>,
    home: Option<&OsStr>,
) -> PathBuf {
    let base = local_app_data.or(app_data).or(home);
    base.map_or_else(
        || PathBuf::from(".").join("Cynapse").join("Client"),
        |base| PathBuf::from(base).join("Cynapse").join("Client"),
    )
}

pub fn global_runtime_path() -> PathBuf {
    global_runtime_dir().join("runtime.json")
}

pub fn global_state_db_path() -> PathBuf {
    global_runtime_dir().join("state.db")
}

pub fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub fn new_discovery(port: u16, workspace: impl Into<String>) -> DiscoveryFile {
    DiscoveryFile {
        workspace: workspace.into(),
        port,
        token: Uuid::new_v4().to_string(),
        pid: std::process::id(),
        started_at: now_unix_seconds(),
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveryHandle {
    path: PathBuf,
    token: String,
}

impl DiscoveryHandle {
    pub fn write(
        path: impl Into<PathBuf>,
        discovery: &DiscoveryFile,
    ) -> Result<Self, DiscoveryError> {
        let path = path.into();
        write_discovery_file(&path, discovery)?;
        Ok(Self {
            path,
            token: discovery.token.clone(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remove(&self) -> Result<(), DiscoveryError> {
        let contents = match std::fs::read(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let Ok(current) = serde_json::from_slice::<DiscoveryFile>(&contents) else {
            // A file we cannot prove is ours stays; the next server replaces it.
            return Ok(());
        };
        if current.token != self.token {
            return Ok(());
        }
        remove_if_exists(&self.path)?;
        Ok(())
    }
}

fn write_discovery_file(path: &Path, discovery: &DiscoveryFile) -> Result<(), DiscoveryError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        secure_directory(parent)?;
    }

    let temporary_path = temporary_discovery_path(path);
    let contents = serde_json::to_vec(discovery)?;
    // Rename replaces the destination atomically on every supported platform,
    // so runtime.json never disappears between writers. A private temporary
    // name keeps two processes from clobbering each other's in-flight file.
    let result = (|| -> Result<(), std::io::Error> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary_path)?;
        file.write_all(&contents)?;
        std::fs::rename(&temporary_path, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    result.map_err(DiscoveryError::from)
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

fn temporary_discovery_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("runtime.json");
    path.with_file_name(format!(
        "{name}.{}.{}.tmp",
        std::process::id(),
        Uuid::new_v4()
    ))
}

fn remove_if_exists(path: &Path) -> Result<(), std::io::Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
