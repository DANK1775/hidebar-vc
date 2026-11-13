//! Fuente de eventos de Niri: habla directamente con el socket IPC (`$NIRI_SOCKET`).

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::event::{Event, Workspaces};

const RETRY: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct NiriWorkspace {
    id: i32,
    idx: Option<i32>,
    output: Option<String>,
    is_focused: bool,
    is_active: bool,
}

#[derive(Deserialize)]
struct WorkspacesChanged {
    workspaces: Vec<NiriWorkspace>,
}

#[derive(Deserialize)]
struct WorkspaceActivated {
    id: i32,
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

fn socket_path() -> Option<PathBuf> {
    std::env::var_os("NIRI_SOCKET").map(PathBuf::from)
}

fn run(path: &PathBuf, emit: &impl Fn(Event) -> bool) -> io::Result<Stop> {
    let mut stream = UnixStream::connect(path)?;
    stream.write_all(b"\"EventStream\"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    // La primera respuesta es {"Ok":"Handled"}; los eventos vienen después, uno por línea.
    let mut last_workspaces: Option<Vec<NiriWorkspace>> = None;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<NiriEvent>(&line) else {
            continue;
        };
        if let Some(changed) = event.workspaces_changed {
            if let Some(workspaces) = workspaces_from_list(&changed.workspaces)
                && !emit(Event::Workspaces(workspaces))
            {
                return Ok(Stop::ReceiverGone);
            }
            last_workspaces = Some(changed.workspaces);
        }
        if let Some(activated) = event.workspace_activated
            && activated.focused
            && let Some(last) = &last_workspaces
        {
            let output = last
                .iter()
                .find(|w| w.id == activated.id)
                .and_then(|w| w.output.clone());
            let selected: Vec<&NiriWorkspace> = match &output {
                Some(output) => last
                    .iter()
                    .filter(|w| w.output.as_deref() == Some(output.as_str()))
                    .collect(),
                None => last.iter().collect(),
            };
            let mut ids: Vec<i32> = selected
                .iter()
                .map(|w| w.idx.unwrap_or(w.id))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            let active = selected
                .iter()
                .find(|w| w.id == activated.id)
                .map(|w| w.idx.unwrap_or(w.id))
                .unwrap_or(activated.id);
            if !emit(Event::Workspaces(Workspaces { ids, active })) {
                return Ok(Stop::ReceiverGone);
            }
        }
    }
    Ok(Stop::Disconnected)
}

/// Extrae el estado de workspaces de una línea de evento. Devuelve `None` si no es
/// `WorkspacesChanged` o el JSON es inválido.
fn workspaces_from_list(workspaces: &[NiriWorkspace]) -> Option<Workspaces> {
    let focused_output = workspaces
        .iter()
        .find(|workspace| workspace.is_focused)
        .and_then(|workspace| workspace.output.clone());

    let selected: Vec<&NiriWorkspace> = match &focused_output {
        Some(output) => workspaces
            .iter()
            .filter(|workspace| workspace.output.as_deref() == Some(output.as_str()))
            .collect(),
        None => workspaces.iter().collect(),
    };

    let mut ids: Vec<i32> = selected
        .iter()
        .map(|workspace| workspace.idx.unwrap_or(workspace.id))
        .collect();
    ids.sort_unstable();
    ids.dedup();

    let active = selected
        .iter()
        .find(|workspace| workspace.is_focused)
        .or_else(|| selected.iter().find(|workspace| workspace.is_active))
        .map(|workspace| workspace.idx.unwrap_or(workspace.id))
        .unwrap_or(0);

    Some(Workspaces { ids, active })
}

#[allow(dead_code)]
pub fn parse_workspaces_event(line: &str) -> Option<Workspaces> {
    let event: NiriEvent = serde_json::from_str(line).ok()?;
    workspaces_from_list(&event.workspaces_changed?.workspaces)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ignores_other_events() {
        let line = r#"{"WindowsChanged":{"windows":[]}}"#;
        assert!(parse_workspaces_event(line).is_none());
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse_workspaces_event("not json").is_none());
    }

    #[test]
    fn uses_idx_fallback() {
        let line = r#"{"WorkspacesChanged":{"workspaces":[{"id":7,"idx":null,"name":null,"output":"eDP-1","is_urgent":false,"is_active":true,"is_focused":true,"active_window_id":null}]}}"#;
        assert_eq!(
            parse_workspaces_event(line).unwrap(),
            Workspaces {
                ids: vec![7],
                active: 7
            }
        );
    }
}
