//! Fuente de eventos de Niri: habla directamente con el socket IPC (`$NIRI_SOCKET`).

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::event::{Event, Workspaces};

const RETRY: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct NiriWorkspace {
    id: u64,
    idx: u8,
    output: Option<String>,
    is_focused: bool,
}

#[derive(Deserialize)]
struct WorkspacesChanged {
    workspaces: Vec<NiriWorkspace>,
}

#[derive(Deserialize)]
struct WorkspaceActivated {
    id: u64,
    focused: bool,
}

#[derive(Deserialize)]
struct NiriEvent {
    #[serde(rename = "WorkspacesChanged")]
    workspaces_changed: Option<WorkspacesChanged>,
    #[serde(rename = "WorkspaceActivated")]
    workspace_activated: Option<WorkspaceActivated>,
}

/// Escucha Niri en un hilo propio y emite el estado de los workspaces. Sin `NIRI_SOCKET` no hace nada.
pub fn listen(emit: impl Fn(Event) -> bool + Send + 'static) {
    let Some(path) = socket_path() else {
        eprintln!("hidebar: NIRI_SOCKET no definido; sin eventos de Niri");
        return;
    };
    std::thread::spawn(move || {
        loop {
            match run(&path, &emit) {
                Ok(Stop::ReceiverGone) => return,
                Ok(Stop::Disconnected) => {}
                Err(error) => eprintln!("hidebar: conexión con Niri: {error}"),
            }
            std::thread::sleep(RETRY);
        }
    });
}

enum Stop {
    ReceiverGone,
    Disconnected,
}

pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("NIRI_SOCKET").map(PathBuf::from)
}

fn run(path: &Path, emit: &impl Fn(Event) -> bool) -> io::Result<Stop> {
    let mut stream = UnixStream::connect(path)?;
    stream.write_all(b"\"EventStream\"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut reply = String::new();
    reader.read_line(&mut reply)?;
    // La primera respuesta es {"Ok":"Handled"}; si niri la rechaza, el flujo nunca traería eventos
    // y este hilo quedaría esperando para siempre, sin reintentar.
    if !reply.contains("\"Ok\"") {
        return Err(io::Error::other(format!(
            "Niri rechazó el flujo de eventos: {}",
            reply.trim()
        )));
    }

    // El estado completo llega al conectar y se reemplaza en cada `WorkspacesChanged`.
    let mut last: Option<Vec<NiriWorkspace>> = None;
    for line in reader.lines() {
        let Ok(event) = serde_json::from_str::<NiriEvent>(&line?) else {
            continue;
        };
        if let Some(changed) = event.workspaces_changed {
            last = Some(changed.workspaces);
        } else if let Some(activated) = event.workspace_activated.filter(|it| it.focused) {
            let Some(workspaces) = &mut last else {
                continue;
            };
            focus(workspaces, activated.id);
        } else {
            continue;
        }
        if let Some(workspaces) = last.as_deref().and_then(workspaces_from_list)
            && !emit(Event::Workspaces(workspaces))
        {
            return Ok(Stop::ReceiverGone);
        }
    }
    Ok(Stop::Disconnected)
}

/// Mueve el foco dentro de la lista en caché: niri no la reenvía al activar un workspace.
fn focus(workspaces: &mut [NiriWorkspace], id: u64) {
    for workspace in workspaces {
        workspace.is_focused = workspace.id == id;
    }
}

/// Reduce la lista de workspaces a lo que importa: los de la salida enfocada, ordenados por índice.
/// `None` mientras ningún workspace esté enfocado (estado transitorio): no hay nada que mostrar.
fn workspaces_from_list(workspaces: &[NiriWorkspace]) -> Option<Workspaces> {
    let focused = workspaces.iter().find(|workspace| workspace.is_focused)?;
    let mut ids: Vec<i32> = workspaces
        .iter()
        .filter(|workspace| workspace.output == focused.output)
        .map(|workspace| i32::from(workspace.idx))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Some(Workspaces {
        ids,
        active: i32::from(focused.idx),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Respuesta de `"Workspaces"`: `{"Ok":{"Workspaces":[…]}}`.
    #[derive(Deserialize)]
    struct WorkspacesReply {
        #[serde(rename = "Ok")]
        ok: ReplyBody,
    }

    #[derive(Deserialize)]
    struct ReplyBody {
        #[serde(rename = "Workspaces")]
        workspaces: Vec<NiriWorkspace>,
    }

    fn parse_workspaces_event(line: &str) -> Option<Workspaces> {
        let event: NiriEvent = serde_json::from_str(line).ok()?;
        workspaces_from_list(&event.workspaces_changed?.workspaces)
    }

    fn workspace(id: u64, idx: u8, output: &str, is_focused: bool) -> NiriWorkspace {
        NiriWorkspace {
            id,
            idx,
            output: Some(output.to_owned()),
            is_focused,
        }
    }

    #[test]
    fn parses_event_line() {
        let line = r#"{"WorkspacesChanged":{"workspaces":[{"id":3,"idx":3,"name":null,"output":"HDMI-A-1","is_urgent":false,"is_active":false,"is_focused":false,"active_window_id":null},{"id":1,"idx":1,"name":"main","output":"HDMI-A-1","is_urgent":false,"is_active":true,"is_focused":true,"active_window_id":11}]}}"#;
        assert_eq!(
            parse_workspaces_event(line).unwrap(),
            Workspaces {
                ids: vec![1, 3],
                active: 1
            }
        );
    }

    #[test]
    fn keeps_only_the_focused_output() {
        let line = r#"{"WorkspacesChanged":{"workspaces":[{"id":1,"idx":1,"output":"HDMI-A-1","is_focused":true},{"id":9,"idx":1,"output":"DP-1","is_focused":false},{"id":10,"idx":2,"output":"DP-1","is_focused":false}]}}"#;
        assert_eq!(
            parse_workspaces_event(line).unwrap(),
            Workspaces {
                ids: vec![1],
                active: 1
            }
        );
    }

    #[test]
    fn waits_until_a_workspace_is_focused() {
        let line = r#"{"WorkspacesChanged":{"workspaces":[{"id":1,"idx":1,"output":"eDP-1","is_focused":false}]}}"#;
        assert!(parse_workspaces_event(line).is_none());
    }

    #[test]
    fn focus_moves_the_dot() {
        let mut workspaces = vec![
            workspace(1, 1, "HDMI-A-1", true),
            workspace(2, 2, "HDMI-A-1", false),
        ];
        focus(&mut workspaces, 2);
        assert_eq!(
            workspaces_from_list(&workspaces).unwrap(),
            Workspaces {
                ids: vec![1, 2],
                active: 2
            }
        );
    }

    #[test]
    fn ignores_other_events() {
        let line = r#"{"WindowsChanged":{"windows":[]}}"#;
        assert!(parse_workspaces_event(line).is_none());
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse_workspaces_event("not json").is_none());
    }

    /// `cargo test -- --ignored`: consulta la sesión real de Niri (sólo lectura).
    #[test]
    #[ignore = "requiere una sesión de Niri"]
    fn queries_live_niri() {
        let path = socket_path().expect("NIRI_SOCKET");
        let mut stream = UnixStream::connect(path).unwrap();
        stream.write_all(b"\"Workspaces\"\n").unwrap();
        let mut reply = String::new();
        // Niri no cierra la conexión tras responder: se lee una línea, no hasta el fin.
        BufReader::new(stream).read_line(&mut reply).unwrap();
        let reply: WorkspacesReply = serde_json::from_str(&reply).unwrap();
        let workspaces = workspaces_from_list(&reply.ok.workspaces).unwrap();
        println!("{workspaces:?}");
        assert!(!workspaces.ids.is_empty());
        assert!(workspaces.ids.contains(&workspaces.active));
    }
}
