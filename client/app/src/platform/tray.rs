//! Tray icon and menu. Menu actions forward events to a channel; the GPUI
//! foreground task performs window effects.

use cynapse_client_i18n::t;
use gpui_kit::*;
use gpui_tray::{Icon, Tray};
use tokio::sync::mpsc::UnboundedSender;

actions!(cynapse_client, [TrayShowMain, TrayShowSettings, TrayQuit]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Activate,
    Settings,
    Quit,
}

struct TrayEvents(UnboundedSender<TrayEvent>);
impl Global for TrayEvents {}

struct ClientTray(Tray);
impl Global for ClientTray {}

pub fn menu_labels() -> (String, String) {
    (t("tray.settings"), t("tray.quit"))
}

fn menu(_cx: &mut App) -> Vec<MenuItem> {
    let (settings_label, quit_label) = menu_labels();
    vec![
        MenuItem::action(settings_label, TrayShowSettings),
        MenuItem::action(quit_label, TrayQuit),
    ]
}

fn send(event: TrayEvent, cx: &mut App) {
    let _ = cx.global::<TrayEvents>().0.send(event);
}

pub fn install(events: UnboundedSender<TrayEvent>, cx: &mut App) -> Result<(), String> {
    let image = image::load_from_memory(include_bytes!("../../assets/icons/icon.png"))
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let icon = Icon::from_rgba(image.as_raw().to_vec(), image.width(), image.height())
        .build()
        .map_err(|error| error.to_string())?;

    cx.set_global(TrayEvents(events));
    cx.on_action(|_: &TrayShowMain, cx| send(TrayEvent::Activate, cx));
    cx.on_action(|_: &TrayShowSettings, cx| send(TrayEvent::Settings, cx));
    cx.on_action(|_: &TrayQuit, cx| send(TrayEvent::Quit, cx));

    let tray = Tray::builder()
        .icon(icon)
        .tooltip("Cynapse Client")
        .menu(menu)
        .on_activate(TrayShowMain)
        .build(cx)
        .map_err(|error| error.to_string())?;
    cx.set_global(ClientTray(tray));
    Ok(())
}

pub fn refresh(cx: &mut App) {
    let Some(tray) = cx.try_global::<ClientTray>().map(|tray| tray.0.clone()) else {
        return;
    };
    if let Err(error) = tray.refresh_menu(cx) {
        tracing::warn!(target: "cynapse.client.tray", %error, "could not refresh tray menu");
    }
    if let Err(error) = tray.set_tooltip(Some("Cynapse Client"), cx) {
        tracing::warn!(target: "cynapse.client.tray", %error, "could not refresh tray tooltip");
    }
}

pub fn close(cx: &mut App) {
    let Some(tray) = cx.try_global::<ClientTray>().map(|tray| tray.0.clone()) else {
        return;
    };
    if let Err(error) = tray.close(cx) {
        tracing::warn!(target: "cynapse.client.tray", %error, "could not close tray");
    }
}
