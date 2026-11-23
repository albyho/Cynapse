//! Win32 window operations GPUI does not expose.
//!
//! Keep every `HWND` in this module; views and the view model work in logical
//! pixels and GPUI window handles only. Functions take a raw `isize` handle so
//! callers can invoke them outside an `App` borrow: synchronous `WM_MOVE` /
//! `WM_SIZE` messages must not re-enter GPUI while it is already borrowed.

use gpui_kit::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWL_STYLE, GetCursorPos, GetWindowLongPtrW, HWND_NOTOPMOST, HWND_TOPMOST, IDYES,
    MB_DEFBUTTON2, MB_ICONQUESTION, MB_TOPMOST, MB_YESNO, MessageBoxW, SW_HIDE, SW_SHOW,
    SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WS_POPUP, WS_THICKFRAME,
};
use windows::core::HSTRING;

/// Extracts the native handle once, while GPUI still has the window.
pub fn raw_hwnd(window: &Window) -> Option<isize> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

fn hwnd(raw: isize) -> Option<HWND> {
    (raw != 0).then_some(HWND(raw as *mut _))
}

fn ex_style(raw: isize) -> Option<isize> {
    let hwnd = hwnd(raw)?;
    Some(unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) })
}

fn set_ex_style(raw: isize, style: isize) {
    if let Some(hwnd) = hwnd(raw) {
        unsafe {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        }
    }
}

/// Makes the window transparent to mouse input and prevents it from taking
/// focus. The caption overlay must not block the Adapter underneath it.
pub fn set_click_through(raw: isize, enabled: bool) {
    let Some(mut style) = ex_style(raw) else {
        return;
    };
    let flags = (WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0 | WS_EX_LAYERED.0) as isize;
    if enabled {
        style |= flags;
    } else {
        style &= !flags;
    }
    set_ex_style(raw, style);
}

/// Removes the native frame Windows adds to the caption overlay.
///
/// The caption window is created as a `PopUp`, but the native style can still
/// carry `WS_CAPTION`/`WS_BORDER`; that draws a 1px frame around the transparent
/// overlay.
pub fn remove_window_border(raw: isize) {
    let Some(hwnd) = hwnd(raw) else {
        return;
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let style = (style
            & !((WS_BORDER.0 | WS_CAPTION.0 | WS_DLGFRAME.0 | WS_THICKFRAME.0) as isize))
            | WS_POPUP.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Returns whether the physical cursor is inside a screen rectangle.
///
/// The caption is click-through, so GPUI never receives hover events for it;
/// the caption view polls this instead.
pub fn cursor_over_rect((left, top, width, height): (i32, i32, i32, i32)) -> bool {
    unsafe {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
        point.x >= left && point.x < left + width && point.y >= top && point.y < top + height
    }
}

/// Shows or restores a window without activating it.
pub fn show_no_activate(raw: isize) {
    if let Some(hwnd) = hwnd(raw) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

pub fn confirm_exit() -> bool {
    let title = HSTRING::from(cynapse_client_i18n::t("dialog.quit.title"));
    let message = HSTRING::from(cynapse_client_i18n::t("dialog.quit.message"));
    let result = unsafe {
        MessageBoxW(
            None,
            &message,
            &title,
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_TOPMOST,
        )
    };
    result == IDYES
}

pub fn show(raw: isize) {
    if let Some(hwnd) = hwnd(raw) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    }
}

pub fn hide(raw: isize) {
    if let Some(hwnd) = hwnd(raw) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

/// Positions and sizes the window in physical pixels. The caller converts from
/// GPUI's logical coordinates at the platform boundary.
pub fn set_physical_bounds(raw: isize, x: i32, y: i32, width: i32, height: i32) {
    if let Some(hwnd) = hwnd(raw) {
        tracing::debug!(
            target: "cynapse.client.caption",
            raw,
            x,
            y,
            width,
            height,
            "SetWindowPos caption bounds"
        );
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
        }
    }
}

pub fn set_topmost(raw: isize, topmost: bool) {
    if let Some(hwnd) = hwnd(raw) {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(if topmost {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                }),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}
