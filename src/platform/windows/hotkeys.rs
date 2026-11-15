//! Atajos globales con `RegisterHotKey`; el shell recibe `WM_HOTKEY`.

use std::io;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey, VK_A, VK_B,
    VK_Q, VK_T, VK_W,
};

use super::logging;
use crate::command::{Command, Target};

const HOTKEYS: [(i32, u32, u16, Command); 9] = [
    (
        1,
        MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
        VK_T,
        Command::Show(Target::Clock),
    ),
    (
        2,
        MOD_CONTROL | MOD_SHIFT | MOD_ALT | MOD_NOREPEAT,
        VK_T,
        Command::Toggle(Target::Clock),
    ),
    (
        3,
        MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
        VK_B,
        Command::Show(Target::Status),
    ),
    (
        4,
        MOD_CONTROL | MOD_SHIFT | MOD_ALT | MOD_NOREPEAT,
        VK_B,
        Command::Toggle(Target::Status),
    ),
    (
        5,
        MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
        VK_W,
        Command::Show(Target::Workspaces),
    ),
    (
        6,
        MOD_CONTROL | MOD_SHIFT | MOD_ALT | MOD_NOREPEAT,
        VK_W,
        Command::Toggle(Target::Workspaces),
    ),
    (
        7,
        MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
        VK_A,
        Command::Show(Target::All),
    ),
    (
        8,
        MOD_CONTROL | MOD_SHIFT | MOD_ALT | MOD_NOREPEAT,
        VK_A,
        Command::Toggle(Target::All),
    ),
    (
        9,
        MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
        VK_Q,
        Command::Quit,
    ),
];

/// Registra todos los atajos posibles. Los que estén ocupados se omiten y se registran los demás.
pub(crate) fn register(hwnd: HWND) -> Vec<i32> {
    let mut registered = Vec::new();

    for &(id, modifiers, key, _) in &HOTKEYS {
        let ok = unsafe { RegisterHotKey(hwnd, id, modifiers, key as u32) } != 0;
        if ok {
            registered.push(id);
        } else {
            logging::error(format!(
                "hidebar: no se pudo registrar el atajo {id}: {}",
                io::Error::last_os_error()
            ));
        }
    }

    registered
}

pub(crate) fn command(id: i32) -> Option<Command> {
    HOTKEYS
        .iter()
        .find(|(hotkey, ..)| *hotkey == id)
        .map(|(_, _, _, command)| *command)
}

pub(crate) fn unregister(hwnd: HWND, registered: &[i32]) {
    for &id in registered {
        unsafe {
            UnregisterHotKey(hwnd, id);
        }
    }
}
