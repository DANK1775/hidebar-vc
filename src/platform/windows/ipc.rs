//! IPC del daemon Windows: named pipe vía `interprocess`, mismo protocolo de texto.
//!
//! El nombre incluye el usuario para no chocar entre cuentas. `HIDEBAR_SOCKET`
//! permite usar otro nombre en desarrollo.

use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    ConnectOptions, GenericNamespaced, ListenerNonblockingMode, ListenerOptions,
};

use super::win32::{SendHwnd, post_event};
use crate::command::Command;
use crate::state::Input;

/// Crea el servidor y atiende conexiones en un hilo propio.
/// Falla si ya hay otro daemon escuchando en el mismo nombre.
pub(crate) fn listen(sender: Sender<Input>, hwnd: SendHwnd) -> io::Result<()> {
    let name = full_name();
    let name = name.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .map_err(|error| {
            if error.kind() == ErrorKind::PermissionDenied {
                io::Error::new(
                    ErrorKind::AddrInUse,
                    "ya hay un daemon escuchando; usa `hidebar quit` para cerrarlo",
                )
            } else {
                error
            }
        })?;
    listener.set_nonblocking(ListenerNonblockingMode::Accept)?;

    thread::spawn(move || {
        loop {
            match listener.accept() {
                Ok(mut stream) => {
                    let mut line = String::new();
                    let read = BufReader::new(&mut stream).read_line(&mut line);
                    let reply = if read.is_err() {
                        "error: no se pudo leer el comando".to_owned()
                    } else {
                        match line.trim().parse::<Command>() {
                            Ok(command) => {
                                if sender.send(Input::Command(command)).is_ok() {
                                    post_event(hwnd);
                                    "ok".to_owned()
                                } else {
                                    "error: el daemon se está cerrando".to_owned()
                                }
                            }
                            Err(message) => format!("error: {message}"),
                        }
                    };

                    let _ = writeln!(stream, "{reply}");
                    let _ = stream.flush();
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    });

    Ok(())
}

pub(crate) fn send(command: Command) -> io::Result<String> {
    let name = full_name();
    let display_name = name.clone();
    let name_ref = name.to_ns_name::<GenericNamespaced>()?;
    let mut stream = ConnectOptions::new()
        .name(name_ref)
        .connect_sync()
        .map_err(|error| {
            if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) {
                io::Error::new(
                    error.kind(),
                    format!(
                        "el daemon no está corriendo ({display_name}); inícialo con `hidebar daemon`"
                    ),
                )
            } else {
                error
            }
        })?;

    writeln!(stream, "{command}")?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    Ok(reply.trim_end().to_owned())
}

fn full_name() -> String {
    if let Ok(name) = std::env::var("HIDEBAR_SOCKET") {
        return name;
    }

    let user = std::env::var("USERNAME")
        .unwrap_or_else(|_| "default".to_owned())
        .replace('\\', "_");

    format!("hidebar-{user}")
}
