//! Client 级用户设置：主题等跨窗口偏好。
//! 这里只定义 Client 设置契约；持久化由 Client App 负责。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Bump when existing settings must be reset; additive fields use serde defaults.
pub const CLIENT_SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePreference {
    #[default]
    System,
    ZhCn,
    En,
}

impl LanguagePreference {
    /// Resolve the interface locale. Any Chinese system language maps to
    /// Simplified Chinese; everything else falls back to English.
    pub fn locale(self, system_language: &str) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
            Self::System => {
                if system_language
                    .trim()
                    .to_ascii_lowercase()
                    .starts_with("zh")
                {
                    "zh-CN"
                } else {
                    "en"
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    #[serde(default)]
    pub version: u32,
    pub language: LanguagePreference,
    pub theme: ThemePreference,
    /// Narration volume in percent. Volume 0 still plays the clip silently.
    pub volume: u8,
    /// Root directory for downloaded Course Packages. `None` uses the
    /// platform default; only absolute directories are accepted.
    pub course_store_dir: Option<PathBuf>,
    pub caption: CaptionSettings,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            version: CLIENT_SETTINGS_VERSION,
            language: LanguagePreference::default(),
            theme: ThemePreference::default(),
            volume: 100,
            course_store_dir: None,
            caption: CaptionSettings::default(),
        }
    }
}

impl ClientSettings {
    /// The custom directory when it is absolute and not an existing file.
    pub fn valid_course_store_dir(&self) -> Option<&Path> {
        self.course_store_dir
            .as_deref()
            .filter(|path| path.is_absolute() && !path.is_file())
    }

    /// The configured course storage root, or the platform default.
    pub fn resolved_course_store_dir(&self) -> PathBuf {
        self.valid_course_store_dir()
            .map_or_else(default_course_store_dir, Path::to_path_buf)
    }

    /// Whether a stored override exists but cannot be used as a directory.
    pub fn has_invalid_course_store_dir(&self) -> bool {
        self.course_store_dir.is_some() && self.valid_course_store_dir().is_none()
    }
}

/// The platform default course storage root.
///
/// Windows uses the user profile rather than the drive root, so no drive
/// detection or elevated permissions are involved. Other platforms use the
/// home directory. The directory is created lazily by the download flow.
fn default_course_store_dir() -> PathBuf {
    let base = user_home_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(std::env::temp_dir);
    base.join("Cynapse").join("Workspace")
}

fn user_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    home.map(PathBuf::from)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionPosition {
    #[default]
    Bottom,
    Top,
}

impl CaptionPosition {
    /// The opposite edge, used by the runtime hover flip.
    pub fn flipped(self) -> Self {
        match self {
            Self::Bottom => Self::Top,
            Self::Top => Self::Bottom,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionFontFamily {
    #[default]
    System,
    MicrosoftYaHei,
    SimHei,
    SimSun,
    DengXian,
}

impl CaptionFontFamily {
    /// The platform font family name; `None` keeps the system UI font.
    pub fn family_name(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::MicrosoftYaHei => Some("Microsoft YaHei"),
            Self::SimHei => Some("SimHei"),
            Self::SimSun => Some("SimSun"),
            Self::DengXian => Some("DengXian"),
        }
    }
}

pub const DEFAULT_CAPTION_TEXT_COLOR: &str = "#ffffff";
pub const DEFAULT_CAPTION_ACCENT_COLOR: &str = "#00ffff";
pub const DEFAULT_CAPTION_BACKGROUND_COLOR: &str = "#000000";

fn default_caption_background_color() -> Option<String> {
    Some(DEFAULT_CAPTION_BACKGROUND_COLOR.into())
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionSettings {
    pub font_size: u16,
    pub font_family: CaptionFontFamily,
    pub text_color: String,
    pub accent_color: String,
    #[serde(default = "default_caption_background_color")]
    pub background_color: Option<String>,
    #[serde(default = "default_true")]
    pub background_enabled: bool,
    pub background_opacity: u8,
    pub text_opacity: u8,
    pub position: CaptionPosition,
    pub width_percent: u8,
}

impl Default for CaptionSettings {
    fn default() -> Self {
        Self {
            font_size: 32,
            font_family: CaptionFontFamily::System,
            text_color: DEFAULT_CAPTION_TEXT_COLOR.into(),
            accent_color: DEFAULT_CAPTION_ACCENT_COLOR.into(),
            background_color: Some(DEFAULT_CAPTION_BACKGROUND_COLOR.into()),
            background_enabled: true,
            background_opacity: 80,
            text_opacity: 100,
            position: CaptionPosition::Bottom,
            width_percent: 80,
        }
    }
}

impl CaptionSettings {
    /// Toggle the background without discarding the chosen color.
    pub fn set_background_enabled(&mut self, enabled: bool) {
        self.background_enabled = enabled;
        if enabled && self.background_color.is_none() {
            self.background_color = Some(DEFAULT_CAPTION_BACKGROUND_COLOR.into());
        }
    }

    /// Keep hand-edited settings inside the ranges the UI exposes.
    pub fn clamped(self) -> Self {
        let background_enabled = self.background_enabled && self.background_color.is_some();
        Self {
            font_size: self.font_size.clamp(20, 48),
            background_enabled,
            background_opacity: self.background_opacity.min(100),
            text_opacity: self.text_opacity.min(100),
            width_percent: self.width_percent.clamp(50, 100),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_carry_the_current_version() {
        assert_eq!(ClientSettings::default().version, CLIENT_SETTINGS_VERSION);
    }

    #[test]
    fn settings_without_a_version_deserialize_to_version_zero() {
        let settings: ClientSettings =
            serde_json::from_str(r#"{"theme":"dark"}"#).expect("settings should deserialize");

        assert_eq!(settings.version, 0);
    }

    #[test]
    fn settings_without_a_caption_font_use_the_system_font() {
        let settings: ClientSettings = serde_json::from_str(
            r#"{
                "theme": "dark",
                "caption": {
                    "font_size": 32,
                    "background_opacity": 50,
                    "position": "bottom",
                    "width_percent": 80
                }
            }"#,
        )
        .expect("old settings should still deserialize");

        assert_eq!(settings.caption.font_family, CaptionFontFamily::System);
        assert_eq!(settings.caption.text_color, DEFAULT_CAPTION_TEXT_COLOR);
        assert_eq!(settings.caption.accent_color, DEFAULT_CAPTION_ACCENT_COLOR);
        assert_eq!(
            settings.caption.background_color.as_deref(),
            Some(DEFAULT_CAPTION_BACKGROUND_COLOR)
        );
        assert!(settings.caption.background_enabled);
        assert_eq!(settings.caption.text_opacity, 100);
        assert_eq!(settings.language, LanguagePreference::System);
        assert_eq!(settings.theme, ThemePreference::Dark);
        assert_eq!(settings.volume, 100);
    }

    #[test]
    fn settings_default_to_full_volume() {
        assert_eq!(ClientSettings::default().volume, 100);
    }

    #[test]
    fn caption_background_defaults_to_eighty_percent_opacity() {
        assert_eq!(ClientSettings::default().caption.background_opacity, 80);
    }

    #[test]
    fn course_store_dir_defaults_under_the_user_home() {
        let path = ClientSettings::default().resolved_course_store_dir();

        assert!(path.is_absolute(), "{path:?}");
        assert!(path.ends_with(Path::new("Cynapse").join("Workspace")));
    }

    #[test]
    fn old_settings_without_a_course_store_dir_use_the_default() {
        let settings: ClientSettings =
            serde_json::from_str(r#"{"version":1}"#).expect("settings should deserialize");

        assert_eq!(settings.course_store_dir, None);
        assert!(!settings.has_invalid_course_store_dir());
    }

    #[test]
    fn absolute_course_store_dir_overrides_the_default() {
        let directory = tempfile::tempdir().unwrap();
        let custom = directory.path().join("store");
        let settings = ClientSettings {
            course_store_dir: Some(custom.clone()),
            ..ClientSettings::default()
        };

        assert_eq!(settings.resolved_course_store_dir(), custom);
        assert!(!settings.has_invalid_course_store_dir());
    }

    #[test]
    fn relative_course_store_dir_is_invalid() {
        let settings = ClientSettings {
            course_store_dir: Some(PathBuf::from("relative")),
            ..ClientSettings::default()
        };

        assert!(settings.has_invalid_course_store_dir());
        assert!(settings.resolved_course_store_dir().is_absolute());
    }

    #[test]
    fn file_course_store_dir_is_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("not-a-directory");
        std::fs::write(&file, "file").unwrap();
        let settings = ClientSettings {
            course_store_dir: Some(file),
            ..ClientSettings::default()
        };

        assert!(settings.has_invalid_course_store_dir());
        assert!(!settings.resolved_course_store_dir().is_file());
    }

    #[test]
    fn caption_position_flips_between_edges() {
        assert_eq!(CaptionPosition::Bottom.flipped(), CaptionPosition::Top);
        assert_eq!(CaptionPosition::Top.flipped(), CaptionPosition::Bottom);
    }

    #[test]
    fn caption_background_color_may_be_absent() {
        let mut settings = ClientSettings::default();
        settings.caption.background_color = None;
        let json = serde_json::to_string(&settings).expect("settings should serialize");
        let restored: ClientSettings =
            serde_json::from_str(&json).expect("settings should deserialize");

        assert_eq!(restored.caption.background_color, None);
        assert!(restored.caption.background_enabled);
        assert!(!restored.caption.clamped().background_enabled);
    }

    #[test]
    fn disabling_the_background_keeps_the_chosen_color() {
        let settings = ClientSettings {
            caption: CaptionSettings {
                background_color: Some("#ff0000".into()),
                background_enabled: false,
                ..CaptionSettings::default()
            },
            ..ClientSettings::default()
        };

        let json = serde_json::to_string(&settings).expect("settings should serialize");
        let restored: ClientSettings =
            serde_json::from_str(&json).expect("settings should deserialize");

        assert_eq!(
            restored.caption.background_color.as_deref(),
            Some("#ff0000")
        );
        assert!(!restored.caption.background_enabled);
    }

    #[test]
    fn toggling_the_background_preserves_the_chosen_color() {
        let mut caption = CaptionSettings {
            background_color: Some("#ff0000".into()),
            ..CaptionSettings::default()
        };

        caption.set_background_enabled(false);
        assert!(!caption.background_enabled);
        assert_eq!(caption.background_color.as_deref(), Some("#ff0000"));

        caption.set_background_enabled(true);
        assert!(caption.background_enabled);
        assert_eq!(caption.background_color.as_deref(), Some("#ff0000"));
    }

    #[test]
    fn language_preferences_use_stable_serde_values() {
        let mut settings = ClientSettings::default();
        assert_eq!(
            serde_json::to_value(&settings).expect("settings should serialize")["language"],
            "system"
        );

        settings.language = LanguagePreference::ZhCn;
        assert_eq!(
            serde_json::to_value(&settings).expect("settings should serialize")["language"],
            "zh_cn"
        );

        settings.language = LanguagePreference::En;
        assert_eq!(
            serde_json::to_value(&settings).expect("settings should serialize")["language"],
            "en"
        );
    }

    #[test]
    fn system_language_maps_chinese_variants_to_simplified_chinese() {
        for system in ["zh", "zh-CN", "zh-Hans", "zh-TW", "zh-HK", "ZH-cn"] {
            assert_eq!(
                LanguagePreference::System.locale(system),
                "zh-CN",
                "{system}"
            );
        }
        for system in ["en", "en-US", "fr", "", "ja-JP"] {
            assert_eq!(LanguagePreference::System.locale(system), "en", "{system}");
        }
    }

    #[test]
    fn explicit_language_choices_ignore_the_system_language() {
        assert_eq!(LanguagePreference::En.locale("zh-CN"), "en");
        assert_eq!(LanguagePreference::ZhCn.locale("en-US"), "zh-CN");
    }
}
