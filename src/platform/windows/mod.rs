//! Shell Windows. Fase 1: stub para que la crate compile en ambas plataformas.
//! La Fase 2 implementa Win32 + `UpdateLayeredWindow` + `iced_tiny_skia`.

mod geometry;
mod render;
mod win32;

use std::io;
use std::process::ExitCode;

use crate::command::Command;

pub(crate) fn run_daemon() -> ExitCode {
    eprintln!("hidebar: el shell de Windows todavía no está implementado");
    ExitCode::FAILURE
}

pub(crate) fn send(_command: Command) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "hidebar para Windows todavía no está implementado",
    ))
}
