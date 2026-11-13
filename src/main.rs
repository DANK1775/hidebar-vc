mod app;
mod audio;
mod battery;
mod event;
mod hyprland;
mod ipc;
mod network;
mod system;

use std::process::ExitCode;

use iced::futures::channel::mpsc;

use crate::app::Input;
use crate::ipc::Command;

const USAGE: &str = "\
uso:
  hidebar daemon            inicia el daemon (exec-once en Hyprland)
  hidebar show <módulo>     muestra unos segundos
  hidebar toggle <módulo>   fija/desfija (clock: abre/cierra el calendario)

módulos: workspaces, clock, status (red, cpu, ram, silencio, batería), all";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("daemon") if args.len() == 1 => run_daemon(),
        Some("-h" | "--help") | None => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(_) => match args.join(" ").parse::<Command>() {
            Ok(command) => run_client(command),
            Err(message) => {
                eprintln!("hidebar: {message}\n\n{USAGE}");
                ExitCode::from(2)
            }
        },
    }
}

fn run_daemon() -> ExitCode {
    let listener = match ipc::bind() {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("hidebar: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (sender, receiver) = mpsc::unbounded();
    let commands = sender.clone();
    ipc::serve(listener, move |command| {
        commands.unbounded_send(Input::Command(command)).is_ok()
    });
    // Cada fuente recibe su propio emisor hacia el mismo canal.
    let emitter = || {
        let events = sender.clone();
        move |event| events.unbounded_send(Input::Event(event)).is_ok()
    };
    hyprland::listen(emitter());
    battery::listen(emitter());
    system::listen(emitter());
    network::listen(emitter());
    audio::listen(emitter());

    let result = app::run(receiver);
    let _ = std::fs::remove_file(ipc::socket_path());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hidebar: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_client(command: Command) -> ExitCode {
    match ipc::send(command) {
        Ok(reply) if reply == "ok" => ExitCode::SUCCESS,
        Ok(reply) => {
            eprintln!("hidebar: {reply}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("hidebar: {error}");
            ExitCode::FAILURE
        }
    }
}
