//! Application asset source: the bundled GPUI Kit icons plus the Cynapse icon.

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

const APP_ICON: &str = "cynapse-icon.png";

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icons/32x32.png"
            ))));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::AllAssets.list(path)?;
        if APP_ICON.starts_with(path) {
            paths.push(APP_ICON.into());
        }
        Ok(paths)
    }
}
