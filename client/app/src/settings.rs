use cynapse_client_core::{
    CLIENT_SETTINGS_VERSION, LanguagePreference, WindowGeometryStore, write_atomic,
};
use cynapse_client_i18n::{t, t_with};
use cynapse_client_model::{ClientSettings, ThemePreference};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, WindowAppearance};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Resolve the interface locale from the user preference and the platform.
pub fn resolve_locale(preference: LanguagePreference) -> &'static str {
    preference.locale(&crate::platform::system_language())
}

/// Apply a resolved locale to both the Client catalog and GPUI components.
pub fn apply_locale(locale: &str) {
    cynapse_client_i18n::set_locale(locale);
    gpui_kit::component::set_locale(locale);
}

/// The previous host used the bundle identifier as the app-data directory. Keep
/// that directory so an in-place migration does not drop the user's settings.
const APP_DIR: &str = "com.cynapse.client";

fn config_dir_from(app_data: Option<&OsStr>) -> Option<PathBuf> {
    app_data.map(PathBuf::from).map(|base| base.join(APP_DIR))
}

fn config_dir() -> Option<PathBuf> {
    config_dir_from(std::env::var_os("APPDATA").as_deref())
}

pub fn settings_path() -> Option<PathBuf> {
    settings_path_from(std::env::var_os("APPDATA").as_deref())
}

fn settings_path_from(app_data: Option<&OsStr>) -> Option<PathBuf> {
    config_dir_from(app_data).map(|dir| dir.join("settings.json"))
}

pub fn window_geometry_store() -> Option<WindowGeometryStore> {
    // The Tauri host stored a tall right-side panel geometry in
    // `main-window-position.json`. Use a host-specific file so the first GPUI
    // launch starts with the intended centered default instead of that layout.
    config_dir().map(|dir| WindowGeometryStore::new(dir.join("gpui-window-position.json")))
}

pub fn save(settings: &ClientSettings) -> Result<(), String> {
    let Some(path) = settings_path() else {
        return Err(t("error.app_data_unavailable"));
    };
    save_to(&path, settings)
}

fn save_to(path: &Path, settings: &ClientSettings) -> Result<(), String> {
    let contents = serde_json::to_vec_pretty(settings).map_err(|error| {
        let detail = error.to_string();
        t_with("error.settings_encode_failed", &[("error", &detail)])
    })?;
    write_atomic(path, &contents).map_err(|error| {
        let path = path.display().to_string();
        let detail = error.to_string();
        t_with(
            "error.settings_write_failed",
            &[("path", &path), ("error", &detail)],
        )
    })
}

pub fn apply_theme(preference: ThemePreference, cx: &mut App) {
    let mode = match preference {
        ThemePreference::System => match cx.window_appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            _ => ThemeMode::Light,
        },
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
    };
    Theme::change(mode, None, cx);
}

pub fn load() -> ClientSettings {
    let Some(path) = settings_path() else {
        return ClientSettings::default();
    };
    load_from(&path)
}

fn load_from(path: &Path) -> ClientSettings {
    match read_settings(path) {
        Ok(Some(settings)) => accept_settings(path, settings),
        Ok(None) => ClientSettings::default(),
        Err(error) => {
            tracing::warn!(
                target: "cynapse.client",
                error = %error,
                "could not read settings; using defaults"
            );
            ClientSettings::default()
        }
    }
}

fn accept_settings(path: &Path, mut settings: ClientSettings) -> ClientSettings {
    if settings.version != CLIENT_SETTINGS_VERSION {
        tracing::warn!(
            target: "cynapse.client",
            path = %path.display(),
            found = settings.version,
            expected = CLIENT_SETTINGS_VERSION,
            "settings version mismatch; using defaults"
        );
        return ClientSettings::default();
    }
    settings.caption = settings.caption.clamped();
    settings.volume = settings.volume.min(100);
    settings
}

fn read_settings(path: &Path) -> Result<Option<ClientSettings>, String> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            let path = path.display().to_string();
            return Err(t_with(
                "error.settings_read_failed",
                &[("path", &path), ("error", &error.to_string())],
            ));
        }
    };

    serde_json::from_str::<ClientSettings>(&contents)
        .map(Some)
        .map_err(|error| {
            let path = path.display().to_string();
            let detail = error.to_string();
            t_with(
                "error.settings_parse_failed",
                &[("path", &path), ("error", &detail)],
            )
        })
}

#[cfg(test)]
mod tests;
