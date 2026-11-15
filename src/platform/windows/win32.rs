//! Helpers Win32 del shell. Todo unsafe está confinado aquí o en `render`.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_TOPMOST, IDC_ARROW, KillTimer, LoadCursorW, PostMessageW, RegisterClassW,
    SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetForegroundWindow, SetTimer,
    SetWindowPos, SetWindowTextW, ShowWindow, WNDCLASSW, WNDPROC, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

pub(crate) const WM_APP_EVENT: u32 = 0x8000 + 1;
pub(crate) const TIMER_ID: usize = 1;

pub(crate) const STRIP_EX_STYLE: u32 =
    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST;

pub(crate) const CALENDAR_EX_STYLE: u32 = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;

pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(null()) }
}

pub(crate) fn register_class(name: &[u16], procedure: WNDPROC) -> bool {
    let class = WNDCLASSW {
        style: 0,
        lpfnWndProc: procedure,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance(),
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
    };

    unsafe { RegisterClassW(&class) != 0 }
}

pub(crate) fn create_window(
    class: &[u16],
    title: &[u16],
    extended_style: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> HWND {
    let hwnd = unsafe {
        CreateWindowExW(
            extended_style,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            width as i32,
            height as i32,
            null_mut(),
            null_mut(),
            instance(),
            null(),
        )
    };
    assert!(!hwnd.is_null(), "CreateWindowExW");

    // CreateWindowExW no dejó el título en el PoC; fijarlo explícitamente.
    unsafe {
        SetWindowTextW(hwnd, title.as_ptr());
    }

    hwnd
}

pub(crate) fn show(hwnd: HWND, activate: bool) {
    unsafe {
        if activate {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        } else {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

pub(crate) fn hide(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
    }
}

pub(crate) fn set_timer(hwnd: HWND, interval: Option<u32>) {
    unsafe {
        match interval {
            Some(milliseconds) => {
                SetTimer(hwnd, TIMER_ID, milliseconds, None);
            }
            None => {
                KillTimer(hwnd, TIMER_ID);
            }
        }
    }
}

pub(crate) fn post_event(hwnd: HWND) {
    unsafe {
        PostMessageW(hwnd, WM_APP_EVENT, 0, 0);
    }
}

pub(crate) fn position_topmost(hwnd: HWND, x: i32, y: i32, width: u32, height: u32) {
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            width as i32,
            height as i32,
            SWP_NOACTIVATE,
        );
    }
}
