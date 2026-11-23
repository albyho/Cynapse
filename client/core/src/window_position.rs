use crate::atomic_file::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

impl WindowPosition {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Persisted main-window geometry. `width` and `height` stay optional so a
/// position-only file written by an older Client still loads and falls back to
/// the configured window size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

impl WindowGeometry {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width: Some(width),
            height: Some(height),
        }
    }

    pub fn position(&self) -> WindowPosition {
        WindowPosition::new(self.x, self.y)
    }

    pub fn size(&self) -> Option<(u32, u32)> {
        match (self.width, self.height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => Some((width, height)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn monitor_containing(
    position: WindowPosition,
    monitors: &[MonitorBounds],
) -> Option<MonitorBounds> {
    monitors.iter().copied().find(|monitor| {
        let right = monitor.x.saturating_add(monitor.width);
        let bottom = monitor.y.saturating_add(monitor.height);

        position.x >= monitor.x
            && position.x < right
            && position.y >= monitor.y
            && position.y < bottom
    })
}

pub fn rect_intersects_monitor(
    position: WindowPosition,
    window_size: (i32, i32),
    monitor: MonitorBounds,
) -> bool {
    let right = position.x.saturating_add(window_size.0.max(1));
    let bottom = position.y.saturating_add(window_size.1.max(1));
    let monitor_right = monitor.x.saturating_add(monitor.width);
    let monitor_bottom = monitor.y.saturating_add(monitor.height);

    position.x < monitor_right
        && right > monitor.x
        && position.y < monitor_bottom
        && bottom > monitor.y
}

/// Returns the position to restore without constraining a window to one
/// monitor. A saved position is kept when any part of the window intersects
/// any current display, so spanning monitors and hanging partly off-screen are
/// valid. Only a window that is completely outside every display is recentered
/// on the primary monitor so it cannot become unreachable.
pub fn restore_position(
    saved: Option<WindowPosition>,
    window_size: (i32, i32),
    monitors: &[MonitorBounds],
    primary: Option<MonitorBounds>,
) -> Option<WindowPosition> {
    let saved = saved?;
    if monitors.is_empty()
        || monitors
            .iter()
            .any(|monitor| rect_intersects_monitor(saved, window_size, *monitor))
    {
        return Some(saved);
    }

    primary
        .or_else(|| monitors.first().copied())
        .map(|monitor| centered_position(window_size, monitor))
}

pub fn centered_position(window_size: (i32, i32), monitor: MonitorBounds) -> WindowPosition {
    WindowPosition::new(
        monitor.x + (monitor.width.saturating_sub(window_size.0)).max(0) / 2,
        monitor.y + (monitor.height.saturating_sub(window_size.1)).max(0) / 2,
    )
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "window position I/O error: {error}"),
            Self::Json(error) => write!(formatter, "window position JSON error: {error}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone)]
pub struct WindowGeometryStore {
    path: PathBuf,
}

impl WindowGeometryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<Option<WindowGeometry>, StoreError> {
        read_geometry(&self.path)
    }

    pub fn save(&self, geometry: WindowGeometry) -> Result<(), StoreError> {
        let contents = serde_json::to_vec(&geometry)?;
        write_atomic(&self.path, &contents)?;
        Ok(())
    }
}

fn read_geometry(path: &Path) -> Result<Option<WindowGeometry>, StoreError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(serde_json::from_str(&contents)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;
