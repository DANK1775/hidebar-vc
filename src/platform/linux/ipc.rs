//! Comunicación cliente → daemon por Unix socket: una línea de texto por comando.

use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use crate::command::Command;

/// `$HIDEBAR_SOCKET` si está definido (p. ej. un daemon de desarrollo junto al habitual);
/// si no, `$XDG_RUNTIME_DIR/hidebar.sock`, o `/tmp/hidebar-$UID.sock`.
pub fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HIDEBAR_SOCKET") {
        return PathBuf::from(path);
    }
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("hidebar.sock"),
        None => {
            // El dueño de /proc/self es el usuario del proceso.
            let uid = std::fs::metadata("/proc/self").map_or(0, |meta| meta.uid());
            PathBuf::from(format!("/tmp/hidebar-{uid}.sock"))
        }
    }
}

/// Envía un comando al daemon y devuelve su respuesta.
pub fn send(command: Command) -> io::Result<String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|error| {
        if matches!(
            error.kind(),
            ErrorKind::NotFound | ErrorKind::ConnectionRefused
        ) {
            io::Error::new(
                error.kind(),
                format!(
                    "el daemon no está corriendo ({}); inícialo con `hidebar daemon`",
                    path.display()
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

/// Crea el socket del daemon. Falla si otro daemon ya lo está usando;
/// elimina un socket huérfano de una ejecución anterior.
pub fn bind() -> io::Result<UnixListener> {
    let path = socket_path();
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err(io::Error::new(
                ErrorKind::AddrInUse,
                format!("ya hay un daemon escuchando en {}", path.display()),
            ));
        }
        std::fs::remove_file(&path)?;
    }
    UnixListener::bind(&path)
}

/// Atiende conexiones en un hilo propio y entrega los comandos válidos a `deliver`,
/// que devuelve `false` cuando el daemon ya no puede recibirlos.
pub fn serve(listener: UnixListener, deliver: impl Fn(Command) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if let Ok(false) = handle(stream, &deliver) {
                break;
            }
        }
    });
}

/// Devuelve si el daemon sigue aceptando comandos.
fn handle(mut stream: UnixStream, deliver: &impl Fn(Command) -> bool) -> io::Result<bool> {
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let mut alive = true;
    let reply = match line.parse::<Command>() {
        Ok(command) => {
            alive = deliver(command);
            if alive {
                "ok".to_owned()
            } else {
                "error: el daemon se está cerrando".to_owned()
            }
        }
        Err(message) => format!("error: {message}"),
    };
    writeln!(stream, "{reply}")?;
    Ok(alive)
}
