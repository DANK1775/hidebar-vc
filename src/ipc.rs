//! Comunicación cliente → daemon por Unix socket: una línea de texto por comando.

use std::fmt;
use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::str::FromStr;

/// Comandos que el daemon entiende.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Muestra unos segundos.
    Show(Target),
    /// El módulo decide: fijar/desfijar, o abrir/cerrar su panel (reloj → calendario).
    Toggle(Target),
}

/// Módulos de la franja, o todos a la vez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Workspaces,
    Clock,
    Status,
    All,
}

impl Target {
    const ALL: [Self; 4] = [Self::Workspaces, Self::Clock, Self::Status, Self::All];

    fn name(self) -> &'static str {
        match self {
            Self::Workspaces => "workspaces",
            Self::Clock => "clock",
            Self::Status => "status",
            Self::All => "all",
        }
    }
}

impl FromStr for Command {
    type Err = String;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let verb: fn(Target) -> Self = match words.first() {
            Some(&"show") => Self::Show,
            Some(&"toggle") => Self::Toggle,
            None => return Err("comando vacío".into()),
            _ => return Err(format!("comando desconocido: {}", words.join(" "))),
        };
        match words.as_slice() {
            [_, name] => Target::ALL
                .into_iter()
                .find(|target| target.name() == *name)
                .map(verb)
                .ok_or_else(|| format!("módulo desconocido: {name}")),
            _ => Err(format!("comando desconocido: {}", words.join(" "))),
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Show(target) => write!(f, "show {}", target.name()),
            Self::Toggle(target) => write!(f, "toggle {}", target.name()),
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_verbs_and_targets() {
        assert_eq!("show clock".parse(), Ok(Command::Show(Target::Clock)));
        assert_eq!(
            "  toggle   status \n".parse(),
            Ok(Command::Toggle(Target::Status))
        );
        assert_eq!("show all".parse(), Ok(Command::Show(Target::All)));
    }

    #[test]
    fn rejects_unknown_and_empty() {
        assert!("show".parse::<Command>().is_err());
        assert!(
            "show battery".parse::<Command>().is_err(),
            "reemplazado por status"
        );
        assert!("hide clock".parse::<Command>().is_err());
        assert!("show workspaces now".parse::<Command>().is_err());
        assert!("\n".parse::<Command>().is_err());
    }

    #[test]
    fn display_round_trips() {
        for target in Target::ALL {
            for command in [Command::Show(target), Command::Toggle(target)] {
                assert_eq!(command.to_string().parse(), Ok(command));
            }
        }
    }
}
