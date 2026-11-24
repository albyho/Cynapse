use crate::settings;
use crate::view_model::ClientViewModel;
use crate::views::caption::CaptionView;
use crate::views::main::MainView;
use crate::views::settings::SettingsView;
use cynapse_client_core::{
    CaptionPosition, MonitorBounds, WindowGeometry, WindowPosition, restore_position,
};
use gpui_kit::component::TitleBar;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub mod single_instance;
pub mod tray;
pub mod window;

const DEFAULT_MAIN_WINDOW_SIZE: (i32, i32) = (420, 640);
const MAIN_WINDOW_EDGE_MARGIN: i32 = 8;
const SETTINGS_WINDOW_SIZE: (f32, f32) = (760.0, 560.0);
use crate::caption_metrics::{CAPTION_MIN_HEIGHT, CAPTION_MIN_WIDTH};

#[cfg(target_os = "windows")]
pub fn system_language() -> String {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;

    let langid = unsafe { GetUserDefaultUILanguage() };
    // LANG_CHINESE; every Chinese variant currently maps to Simplified Chinese.
    if langid & 0x3ff == 0x04 {
        "zh".to_string()
    } else {
        "en".to_string()
    }
}

#[cfg(not(target_os = "windows"))]
pub fn system_language() -> String {
    "en".to_string()
}

#[derive(Clone, Copy)]
pub struct MainWindow {
    pub handle: AnyWindowHandle,
    pub hwnd: isize,
}

/// Reports a startup error without depending on a Windows-only dialog API.
/// Windows shows a native message box; other platforms write to the log/console
/// until a native cross-platform dialog host is added.
pub fn show_error(message: &str) {
    #[cfg(windows)]
    {
        // Test seam: the startup smoke test must exercise the exit path without
        // blocking forever on a native modal dialog.
        if std::env::var_os("CYNAPSE_TEST_NO_DIALOG").is_some() {
            tracing::error!(target: "cynapse.client", "{message}");
            return;
        }

        use windows::Win32::UI::WindowsAndMessaging::{
            MB_ICONERROR, MB_OK, MB_TOPMOST, MessageBoxW,
        };
        use windows::core::HSTRING;

        let title = HSTRING::from("Cynapse Client");
        let message = HSTRING::from(message);
        unsafe {
            let _ = MessageBoxW(None, &message, &title, MB_OK | MB_ICONERROR | MB_TOPMOST);
        }
    }
    #[cfg(not(windows))]
    {
        tracing::error!(target: "cynapse.client", "{message}");
        eprintln!("Cynapse Client: {message}");
    }
}

/// Opens the main window around an existing view model.
///
/// Window creation and platform effects live here, not in the view model or
/// the declarative view.
pub fn open_main_window(cx: &mut App, view_model: Entity<ClientViewModel>) -> MainWindow {
    let saved = settings::window_geometry_store().and_then(|store| store.load().ok().flatten());
    let bounds = initial_bounds(cx, saved);
    let mut options = TitleBar::window_options();
    options.window_bounds = Some(bounds);
    options.window_min_size = Some(size(
        px(DEFAULT_MAIN_WINDOW_SIZE.0 as f32),
        px(DEFAULT_MAIN_WINDOW_SIZE.1 as f32),
    ));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("Cynapse Client".into());
    }

    let mut hwnd = 0isize;
    let (handle, _) = gpui_kit::open_window(options, cx, |window, cx| {
        hwnd = window::raw_hwnd(window).unwrap_or(0);
        window.on_window_should_close(cx, |window, cx| {
            save_window_bounds(window);
            if let Some(hwnd) = window::raw_hwnd(window) {
                cx.spawn(async move |_| window::hide(hwnd)).detach();
            }
            false
        });
        cx.new(|cx| MainView::new(view_model, window, cx))
    })
    .expect("failed to open the main window");

    MainWindow { handle, hwnd }
}

pub fn show_main_window(main: MainWindow) {
    window::show(main.hwnd);
}

pub fn show_main_window_no_activate(main: MainWindow) {
    window::show_no_activate(main.hwnd);
}

pub fn set_main_window_topmost(main: MainWindow, topmost: bool) {
    window::set_topmost(main.hwnd, topmost);
}

#[derive(Clone, Copy)]
pub struct SettingsWindow {
    pub handle: AnyWindowHandle,
    pub hwnd: isize,
}

pub fn open_settings_window(cx: &mut App, view_model: Entity<ClientViewModel>) -> SettingsWindow {
    let mut options = TitleBar::window_options();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(
        None,
        size(px(SETTINGS_WINDOW_SIZE.0), px(SETTINGS_WINDOW_SIZE.1)),
        cx,
    )));
    options.window_min_size = Some(size(px(600.), px(440.)));
    options.show = false;
    options.focus = false;
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(cynapse_client_i18n::t("window.settings.title").into());
    }

    let mut hwnd = 0isize;
    let (handle, _) = gpui_kit::open_window(options, cx, |window, cx| {
        hwnd = window::raw_hwnd(window).unwrap_or(0);
        window.on_window_should_close(cx, |window, cx| {
            if let Some(hwnd) = window::raw_hwnd(window) {
                cx.spawn(async move |_| window::hide(hwnd)).detach();
            }
            false
        });
        cx.new(|cx| SettingsView::new(view_model, cx))
    })
    .expect("failed to open the settings window");

    SettingsWindow { handle, hwnd }
}

pub fn show_settings_window(settings: SettingsWindow) {
    window::show(settings.hwnd);
}

#[derive(Clone, Copy)]
pub struct CaptionWindow {
    hwnd: isize,
}

/// Opens the caption overlay hidden. The platform layer shows it with
/// `ShowWindow` and positions it on the main window's monitor.
pub fn open_caption_window(cx: &mut App, view_model: Entity<ClientViewModel>) -> CaptionWindow {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(CAPTION_MIN_WIDTH), px(CAPTION_MIN_HEIGHT)),
        })),
        kind: WindowKind::PopUp,
        focus: false,
        show: false,
        window_background: WindowBackgroundAppearance::Transparent,
        titlebar: None,
        is_resizable: false,
        is_movable: false,
        ..Default::default()
    };

    let mut hwnd = 0isize;
    cx.open_window(options, |window, cx| {
        hwnd = window::raw_hwnd(window).unwrap_or(0);
        // The caption is hide-only; never let WM_CLOSE destroy it.
        window.on_window_should_close(cx, |_, _| false);
        cx.new(|cx| CaptionView::new(view_model, cx))
    })
    .expect("failed to open the caption window");

    // Configure the permanent caption styles after the current window-creation
    // update finishes. Applying them synchronously here re-enters GPUI through
    // WM_MOVE/WM_SIZE while the App is still mutably borrowed.
    let caption = CaptionWindow { hwnd };
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(0))
            .await;
        window::remove_window_border(hwnd);
        window::set_click_through(hwnd, true);
        window::set_topmost(hwnd, true);
        tracing::debug!(
            target: "cynapse.client.caption",
            hwnd,
            "caption window styles initialized"
        );
    })
    .detach();

    caption
}

/// Computes the caption rectangle in physical pixels on the main window's
/// current monitor. Keep this separate from the Win32 calls: the caller must
/// release the `App` borrow before showing the window, otherwise the
/// synchronous `WM_MOVE`/`WM_SIZE` handlers re-enter GPUI.
pub fn caption_rect(
    cx: &mut App,
    main: AnyWindowHandle,
    position: CaptionPosition,
    width_percent: u8,
    measured_width: u32,
    measured_height: u32,
) -> Option<(i32, i32, i32, i32)> {
    let (visible, scale) = main
        .update(cx, |_, window, cx| {
            window
                .display(cx)
                .map(|display| (display.visible_bounds(), window.scale_factor()))
        })
        .ok()
        .flatten()?;
    tracing::debug!(
        target: "cynapse.client.caption",
        ?visible,
        scale,
        width_percent,
        measured_width,
        measured_height,
        ?position,
        "caption rect inputs"
    );

    let (x, y, width, height) = crate::caption_metrics::caption_rect_logical(
        (visible.origin.x.as_f32(), visible.origin.y.as_f32()),
        (visible.size.width.as_f32(), visible.size.height.as_f32()),
        width_percent,
        (measured_width, measured_height),
        position,
    );
    let scale = scale.max(0.1);
    let to_physical = |value: f32| (value * scale).round() as i32;

    let rect = (
        to_physical(x),
        to_physical(y),
        to_physical(width),
        to_physical(height),
    );
    tracing::debug!(
        target: "cynapse.client.caption",
        ?rect,
        "caption rect computed"
    );
    Some(rect)
}

/// Measure the current caption against the main window's display and store the
/// resulting size on the view model. Called BEFORE the caption window is shown,
/// so the first visible frame is already at the correct size (no flash).
pub fn measure_caption_size(
    cx: &mut App,
    main: AnyWindowHandle,
    view_model: &Entity<crate::view_model::ClientViewModel>,
) {
    let params = {
        let vm = view_model.read(cx);
        crate::caption_metrics::CaptionMeasureParams {
            text: vm.caption_text().to_owned(),
            font_size: vm.caption_font_size(),
            font_family: vm.caption_font_family().family_name().map(str::to_owned),
            text_color: vm.caption_text_color().to_owned(),
            accent_color: vm.caption_accent_color().to_owned(),
            width_percent: vm.caption_width_percent(),
        }
    };
    let measured = main.update(cx, |_, window, cx| {
        crate::views::caption::measure_caption(&params, window, cx)
    });
    if let Ok(Some((width, height))) = measured {
        view_model.update(cx, |vm, _| vm.set_caption_size_silent(width, height));
        tracing::debug!(
            target: "cynapse.client.caption",
            width,
            height,
            "caption measured before show"
        );
    }
}

/// Shows and positions the caption. Call this with no `App` borrow held.
pub fn show_caption_window(caption: CaptionWindow, rect: (i32, i32, i32, i32)) {
    let (x, y, width, height) = rect;
    tracing::debug!(
        target: "cynapse.client.caption",
        hwnd = caption.hwnd,
        x,
        y,
        width,
        height,
        "showing caption window"
    );
    window::set_physical_bounds(caption.hwnd, x, y, width, height);
    window::show_no_activate(caption.hwnd);
}

/// Hides the caption. Call this with no `App` borrow held.
pub fn hide_caption_window(caption: CaptionWindow) {
    tracing::debug!(
        target: "cynapse.client.caption",
        hwnd = caption.hwnd,
        "hiding caption window"
    );
    window::hide(caption.hwnd);
}

/// Persists the current normal-window bounds. Maximized and fullscreen states
/// keep the previously saved restore bounds.
pub fn save_main_window_bounds(cx: &mut App, handle: AnyWindowHandle) {
    if handle
        .update(cx, |_, window, _| save_window_bounds(window))
        .is_err()
    {
        tracing::debug!(target: "cynapse.client", "main window is already closed");
    }
}

fn save_window_bounds(window: &Window) {
    let Some(store) = settings::window_geometry_store() else {
        return;
    };
    let WindowBounds::Windowed(bounds) = window.window_bounds() else {
        return;
    };
    if let Err(error) = store.save(WindowGeometry::new(
        bounds.origin.x.as_f32().round() as i32,
        bounds.origin.y.as_f32().round() as i32,
        bounds.size.width.as_f32().round().max(1.0) as u32,
        bounds.size.height.as_f32().round().max(1.0) as u32,
    )) {
        tracing::warn!(
            target: "cynapse.client",
            %error,
            "could not save the main-window bounds"
        );
    }
}

fn initial_bounds(cx: &App, saved: Option<WindowGeometry>) -> WindowBounds {
    let monitors: Vec<MonitorBounds> = cx
        .displays()
        .iter()
        .map(|display| monitor_bounds(display.visible_bounds()))
        .collect();
    let primary = cx
        .primary_display()
        .map(|display| monitor_bounds(display.visible_bounds()));
    let default_monitor = primary.or_else(|| monitors.first().copied());
    let (width, height) = saved
        .and_then(|geometry| geometry.size())
        .map(|(width, height)| (width as i32, height as i32))
        .or_else(|| {
            default_monitor.map(|monitor| {
                (
                    DEFAULT_MAIN_WINDOW_SIZE.0,
                    (monitor.height - MAIN_WINDOW_EDGE_MARGIN * 2).max(DEFAULT_MAIN_WINDOW_SIZE.1),
                )
            })
        })
        .unwrap_or(DEFAULT_MAIN_WINDOW_SIZE);
    let width = width.max(DEFAULT_MAIN_WINDOW_SIZE.0);
    let height = height.max(DEFAULT_MAIN_WINDOW_SIZE.1);
    let restored = restore_position(
        saved.map(|geometry| geometry.position()),
        (width, height),
        &monitors,
        primary,
    )
    .or_else(|| default_monitor.map(|monitor| right_aligned_position((width, height), monitor)));

    let window_size = size(px(width as f32), px(height as f32));
    match restored {
        Some(WindowPosition { x, y }) => WindowBounds::Windowed(Bounds {
            origin: point(px(x as f32), px(y as f32)),
            size: window_size,
        }),
        None => WindowBounds::Windowed(Bounds::centered(None, window_size, cx)),
    }
}

/// First launch is a right-side panel spanning the monitor work area, so it
/// never covers the taskbar.
fn right_aligned_position(window_size: (i32, i32), monitor: MonitorBounds) -> WindowPosition {
    WindowPosition::new(
        monitor.x
            + monitor
                .width
                .saturating_sub(window_size.0 + MAIN_WINDOW_EDGE_MARGIN),
        monitor.y + MAIN_WINDOW_EDGE_MARGIN,
    )
}

fn monitor_bounds(bounds: Bounds<Pixels>) -> MonitorBounds {
    MonitorBounds {
        x: bounds.origin.x.as_f32().round() as i32,
        y: bounds.origin.y.as_f32().round() as i32,
        width: bounds.size.width.as_f32().round() as i32,
        height: bounds.size.height.as_f32().round() as i32,
    }
}

#[cfg(test)]
mod tests;
