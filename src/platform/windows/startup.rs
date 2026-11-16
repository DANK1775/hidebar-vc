//! Autoarranque en `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Sin admin y sin tocar el sistema: una sola clave por usuario.

use std::process::ExitCode;

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
};
use windows::core::HSTRING;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "hidebar";

pub(crate) fn install() -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            eprintln!("hidebar: no se pudo resolver el ejecutable: {error}");
            return ExitCode::FAILURE;
        }
    };

    let command = format!("\"{}\" daemon", exe.display());
    match write(&command) {
        Ok(()) => {
            println!("hidebar: instalado en el inicio de sesión ({command})");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("hidebar: no se pudo instalar el autoarranque: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn uninstall() -> ExitCode {
    match remove() {
        Ok(()) => {
            println!("hidebar: autoarranque eliminado");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("hidebar: no se pudo quitar el autoarranque: {error}");
            ExitCode::FAILURE
        }
    }
}

fn write(command: &str) -> windows::core::Result<()> {
    let key = open()?;
    let utf16: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `utf16` vive hasta que termina `RegSetValueExW`; el tamaño en bytes
    // corresponde a `utf16.len() * size_of::<u16>()`.
    let bytes = unsafe { std::slice::from_raw_parts(utf16.as_ptr().cast(), utf16.len() * 2) };

    // SAFETY: `key` está abierta; el nombre y los datos viven durante la llamada.
    let result =
        unsafe { RegSetValueExW(key, &HSTRING::from(VALUE_NAME), None, REG_SZ, Some(bytes)) };
    // SAFETY: `key` es válida y no se usa después de cerrarla.
    unsafe {
        let _ = RegCloseKey(key);
    }
    result.ok()
}

fn remove() -> windows::core::Result<()> {
    let key = open()?;
    // SAFETY: `key` está abierta y el nombre vive durante la llamada.
    let result = unsafe { RegDeleteValueW(key, &HSTRING::from(VALUE_NAME)) };
    // SAFETY: `key` es válida y no se usa después.
    unsafe {
        let _ = RegCloseKey(key);
    }
    result.ok()
}

fn open() -> windows::core::Result<HKEY> {
    let mut key = HKEY::default();

    // SAFETY: `HKEY_CURRENT_USER` es válido; el nombre y la clase viven durante
    // la llamada; `key` es un out inicializado por la API.
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    result.ok()?;

    Ok(key)
}
