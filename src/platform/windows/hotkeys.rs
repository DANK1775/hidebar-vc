//! Atajos globales con `RegisterHotKey`; el shell recibe `WM_HOTKEY`.

use std::io;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_WIN, RegisterHotKey, UnregisterHotKey, VK_1, VK_2,
    VK_3, VK_4, VK_A, VK_B, VK_T, VK_W,
};

use super::logging;
use crate::command::{Command, Target};

/// Ctrl+Alt+Win no es AltGr y no pisa reabrir pestaña, cerrar ventana ni
/// marcadores. Mostrar usa letras; fijar usa 1=reloj, 2=estado, 3=workspaces,
/// 4=todo. No hay atajo para salir: `hidebar quit`.
const HOTKEYS: [(i32, u32, u16, Command); 8] = [
    (
        1,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_T,
        Command::Show(Target::Clock),
    ),
    (
        2,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_1,
        Command::Toggle(Target::Clock),
    ),
    (
        3,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_B,
        Command::Show(Target::Status),
    ),
    (
        4,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_2,
        Command::Toggle(Target::Status),
    ),
    (
        5,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_W,
        Command::Show(Target::Workspaces),
    ),
    (
        6,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_3,
        Command::Toggle(Target::Workspaces),
    ),
    (
        7,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_A,
        Command::Show(Target::All),
    ),
    (
        8,
        MOD_CONTROL | MOD_ALT | MOD_WIN | MOD_NOREPEAT,
        VK_4,
        Command::Toggle(Target::All),
    ),
];

/// Registra todos los atajos posibles. Los que estén ocupados se omiten y se registran los demás.
/// `HIDEBAR_NO_HOTKEYS=1` desactiva todos sin recompilar.
pub(crate) fn register(hwnd: HWND) -> Vec<i32> {
    if std::env::var_os("HIDEBAR_NO_HOTKEYS").is_some() {
        return Vec::new();
    }

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
