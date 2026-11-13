//! Daemon gráfico (ADR-0001, ADR-0002). Superficies, todas sin zona exclusiva:
//!
//! - **Franja** superior, click-through: una pastilla por módulo visible
//!   (`workspaces` a la izquierda, `clock` al centro, `status` a la derecha).
//! - **Indicador**, click-through: punto que pulsa mientras la batería está baja y `status` no se ve.
//! - **Calendario**, interactivo: se abre con `toggle clock`, debajo del reloj.
//!
//! Cada módulo está oculto, momentáneo (con vencimiento) o fijado. `sync` abre y cierra las superficies
//! según ese estado después de cada mensaje.

mod calendar;
mod view;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use iced::futures::channel::mpsc::UnboundedReceiver;
use iced::keyboard::{self, key::Named};
use iced::{Color, Subscription, Task, event, window};
use iced_layershell::reexport::{KeyboardInteractivity, Layer, NewLayerShellSettings};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::{daemon, to_layer_message};

use crate::event::{Audio, Battery, Event, Network, System, Workspaces};
use crate::ipc::{Command, Target};

const NAMESPACE: &str = "hidebar";
/// Tiempo visible de cada módulo en modo momentáneo; volver a pedirlo lo reinicia.
const WORKSPACES_DURATION: Duration = Duration::from_millis(1500);
const CLOCK_DURATION: Duration = Duration::from_secs(3);
const STATUS_DURATION: Duration = Duration::from_secs(3);
/// Revisión de vencimientos mientras hay algo momentáneo.
const TICK: Duration = Duration::from_millis(250);
/// Refresco de la hora con el reloj fijado y nada momentáneo.
const CLOCK_TICK: Duration = Duration::from_secs(1);
/// Intervalo de cuadros mientras algo pulsa (20 fps: suficiente para un fade lento).
const FRAME: Duration = Duration::from_millis(50);

/// Umbrales de batería (descargando), en porcentaje.
const BATTERY_LOW: u8 = 25;
const BATTERY_CRITICAL: u8 = 10;
/// Pulso de batería baja: la opacidad baja hasta `1 - PULSE_DIM` y vuelve, cada `PULSE_PERIOD`.
const PULSE_PERIOD: Duration = Duration::from_secs(2);
const PULSE_DIM: f32 = 0.7;

/// Lo que llega al daemon desde fuera de iced: comandos del cliente y eventos de las fuentes.
#[derive(Debug, Clone)]
pub enum Input {
    Command(Command),
    Event(Event),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Module {
    Workspaces,
    Clock,
    Status,
}

const MODULES: [Module; 3] = [Module::Workspaces, Module::Clock, Module::Status];

impl Module {
    fn duration(self) -> Duration {
        match self {
            Self::Workspaces => WORKSPACES_DURATION,
            Self::Clock => CLOCK_DURATION,
            Self::Status => STATUS_DURATION,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visibility {
    Hidden,
    Until(Instant),
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Normal,
    Low,
    Critical,
}

/// Calendario abierto.
struct Calendar {
    /// Primer día del mes mostrado.
    month: NaiveDate,
    /// El reloj ya estaba fijado al abrir: al cerrar el calendario se respeta.
    clock_was_pinned: bool,
    /// Recibió el foco alguna vez; sólo entonces perderlo lo cierra.
    focused: bool,
}

struct Hidebar {
    visibility: [Visibility; 3],
    /// Últimos datos conocidos; `None` hasta el primer evento de cada fuente.
    workspaces: Option<Workspaces>,
    battery: Option<Battery>,
    system: Option<System>,
    network: Option<Network>,
    audio: Option<Audio>,
    calendar: Option<Calendar>,
    /// Superficies abiertas.
    strip: Option<window::Id>,
    indicator: Option<window::Id>,
    calendar_surface: Option<window::Id>,
    /// Origen común del pulso (indicador e icono van en fase) y último cuadro.
    pulse_origin: Instant,
    frame: Instant,
    now: Now,
}

/// Hora y fecha ya formateadas para mostrar, y el día de hoy.
struct Now {
    time: String,
    date: String,
    today: NaiveDate,
}

impl Hidebar {
    fn new() -> Self {
        Self {
            visibility: [Visibility::Hidden; 3],
            workspaces: None,
            battery: None,
            system: None,
            network: None,
            audio: None,
            calendar: None,
            strip: None,
            indicator: None,
            calendar_surface: None,
            pulse_origin: Instant::now(),
            frame: Instant::now(),
            now: now(),
        }
    }

    fn visibility(&self, module: Module) -> Visibility {
        self.visibility[module as usize]
    }

    fn set_visibility(&mut self, module: Module, visibility: Visibility) {
        self.visibility[module as usize] = visibility;
    }

    fn visible(&self, module: Module) -> bool {
        self.visibility(module) != Visibility::Hidden
    }

    /// Sin una lectura de la fuente no hay nada que mostrar.
    fn available(&self, module: Module) -> bool {
        match module {
            Module::Workspaces => self.workspaces.is_some(),
            Module::Clock => true,
            Module::Status => {
                self.battery.is_some()
                    || self.system.is_some()
                    || self.network.is_some()
                    || self.audio.is_some()
            }
        }
    }

    /// Momentáneo, salvo que esté fijado.
    fn show(&mut self, module: Module) {
        if self.available(module) && self.visibility(module) != Visibility::Pinned {
            if module == Module::Clock {
                self.now = now();
            }
            self.set_visibility(
                module,
                Visibility::Until(Instant::now() + module.duration()),
            );
        }
    }

    fn toggle(&mut self, module: Module) {
        if module == Module::Clock {
            return self.toggle_calendar();
        }
        let next = match self.visibility(module) {
            Visibility::Pinned => Visibility::Hidden,
            _ if self.available(module) => Visibility::Pinned,
            current => current,
        };
        self.set_visibility(module, next);
    }

    /// ADR-0002 Q2: si algo está fijado, desfija todo; si no, fija los tres (el reloj, sin calendario).
    fn toggle_all(&mut self) {
        let any_pinned = MODULES
            .iter()
            .any(|module| self.visibility(*module) == Visibility::Pinned);
        if any_pinned {
            self.calendar = None;
            for module in MODULES {
                if self.visibility(module) == Visibility::Pinned {
                    self.set_visibility(module, Visibility::Hidden);
                }
            }
        } else {
            self.now = now();
            for module in MODULES {
                if self.available(module) {
                    self.set_visibility(module, Visibility::Pinned);
                }
            }
        }
    }

    fn toggle_calendar(&mut self) {
        if self.calendar.is_some() {
            return self.close_calendar();
        }
        self.now = now();
        self.calendar = Some(Calendar {
            month: calendar::first_of_month(self.now.today),
            clock_was_pinned: self.visibility(Module::Clock) == Visibility::Pinned,
            focused: false,
        });
        self.set_visibility(Module::Clock, Visibility::Pinned);
    }

    fn close_calendar(&mut self) {
        if let Some(calendar) = self.calendar.take()
            && !calendar.clock_was_pinned
        {
            self.set_visibility(Module::Clock, Visibility::Hidden);
        }
    }

    fn expire(&mut self, instant: Instant) {
        for visibility in &mut self.visibility {
            if matches!(visibility, Visibility::Until(until) if *until <= instant) {
                *visibility = Visibility::Hidden;
            }
        }
    }

    fn severity(&self) -> Severity {
        self.battery.as_ref().map_or(Severity::Normal, severity)
    }

    /// Opacidad actual del pulso.
    fn pulse(&self) -> f32 {
        pulse_alpha(self.frame.saturating_duration_since(self.pulse_origin))
    }

    /// Hay algo pulsando en pantalla: el indicador, o la batería baja dentro de `status`.
    fn pulsing(&self) -> bool {
        self.indicator.is_some()
            || (self.strip.is_some()
                && self.visible(Module::Status)
                && self.severity() != Severity::Normal)
    }
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
enum Message {
    Input(Input),
    Tick,
    Frame,
    Closed(window::Id),
    Focus(window::Id, bool),
    CalendarMonth(i32),
    CalendarToday,
    CalendarClose,
}

pub fn run(inputs: UnboundedReceiver<Input>) -> Result<(), iced_layershell::Error> {
    let inputs = Mutex::new(Some(inputs));
    daemon(
        move || {
            let task = match inputs.lock().ok().and_then(|mut slot| slot.take()) {
                Some(receiver) => Task::run(receiver, Message::Input),
                None => Task::none(),
            };
            (Hidebar::new(), task)
        },
        || NAMESPACE.to_owned(),
        update,
        view::view,
    )
    .subscription(subscription)
    .style(|_, _| iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: Color::WHITE,
    })
    .settings(Settings {
        layer_settings: LayerShellSettings {
            // Sin superficie inicial: sólo se dibuja cuando se pide.
            start_mode: StartMode::Background,
            layer: Layer::Overlay,
            exclusive_zone: -1,
            keyboard_interactivity: KeyboardInteractivity::None,
            events_transparent: true,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}

fn update(state: &mut Hidebar, message: Message) -> Task<Message> {
    let task = handle(state, message);
    Task::batch([task, sync(state)])
}

fn handle(state: &mut Hidebar, message: Message) -> Task<Message> {
    match message {
        Message::Input(Input::Command(command)) => match command {
            Command::Show(target) => modules(target)
                .iter()
                .for_each(|module| state.show(*module)),
            Command::Toggle(Target::All) => state.toggle_all(),
            Command::Toggle(target) => modules(target)
                .iter()
                .for_each(|module| state.toggle(*module)),
        },
        Message::Input(Input::Event(event)) => on_event(state, event),
        Message::Tick => {
            state.now = now();
            state.frame = Instant::now();
            state.expire(state.frame);
        }
        Message::Frame => state.frame = Instant::now(),
        Message::Closed(id) => {
            // Si el compositor cerró una superficie, `sync` la vuelve a abrir si sigue haciendo falta.
            state.strip.take_if(|strip| *strip == id);
            state.indicator.take_if(|indicator| *indicator == id);
            if state
                .calendar_surface
                .take_if(|surface| *surface == id)
                .is_some()
            {
                state.close_calendar();
            }
        }
        Message::Focus(id, focused) => {
            if state.calendar_surface == Some(id)
                && let Some(calendar) = &mut state.calendar
            {
                if focused {
                    calendar.focused = true;
                } else if calendar.focused {
                    // ADR-0002 Q3: perder el foco (clic en otra ventana) lo cierra.
                    state.close_calendar();
                }
            }
        }
        Message::CalendarMonth(delta) => {
            if let Some(calendar) = &mut state.calendar {
                calendar.month = calendar::shift_month(calendar.month, delta);
            }
        }
        Message::CalendarToday => {
            let today = calendar::first_of_month(state.now.today);
            if let Some(calendar) = &mut state.calendar {
                calendar.month = today;
            }
        }
        Message::CalendarClose => state.close_calendar(),
        // Las variantes de layer-shell generadas por la macro las consume el runtime.
        _ => {}
    }
    Task::none()
}

/// Módulos a los que se dirige un comando.
fn modules(target: Target) -> &'static [Module] {
    match target {
        Target::Workspaces => &[Module::Workspaces],
        Target::Clock => &[Module::Clock],
        Target::Status => &[Module::Status],
        Target::All => &MODULES,
    }
}

/// La presentación decide qué eventos merecen aparecer.
fn on_event(state: &mut Hidebar, event: Event) {
    match event {
        Event::Workspaces(workspaces) => {
            let switched = state
                .workspaces
                .as_ref()
                .is_some_and(|old| old.active != workspaces.active);
            state.workspaces = Some(workspaces);
            if switched {
                state.show(Module::Workspaces);
            }
        }
        Event::Battery(battery) => {
            // Conectar o desconectar el cargador se muestra; el nivel bajo lo cubre el indicador.
            let plug_changed = state
                .battery
                .as_ref()
                .is_some_and(|old| old.plugged() != battery.plugged());
            state.battery = Some(battery);
            if plug_changed {
                state.show(Module::Status);
            }
        }
        Event::System(system) => state.system = Some(system),
        Event::Network(network) => {
            let changed = state.network.is_some_and(|old| old != network);
            state.network = Some(network);
            if changed {
                state.show(Module::Status);
            }
        }
        Event::Audio(audio) => {
            // Sólo interesa si se oye o no; los cambios de volumen no aparecen.
            let changed = state
                .audio
                .is_some_and(|old| old.silent() != audio.silent());
            state.audio = Some(audio);
            if changed {
                state.show(Module::Status);
            }
        }
    }
}

/// Abre y cierra las superficies según el estado.
fn sync(state: &mut Hidebar) -> Task<Message> {
    let strip = MODULES.iter().any(|module| state.visible(*module));
    let indicator = state.severity() != Severity::Normal && !state.visible(Module::Status);
    let calendar = state.calendar.is_some();
    Task::batch([
        surface(&mut state.strip, strip, view::strip_settings),
        surface(&mut state.indicator, indicator, view::indicator_settings),
        surface(
            &mut state.calendar_surface,
            calendar,
            view::calendar_settings,
        ),
    ])
}

fn surface(
    slot: &mut Option<window::Id>,
    wanted: bool,
    settings: fn() -> NewLayerShellSettings,
) -> Task<Message> {
    match (wanted, *slot) {
        (true, None) => {
            let id = window::Id::unique();
            *slot = Some(id);
            Task::done(Message::NewLayerShell {
                settings: settings(),
                id,
            })
        }
        (false, Some(id)) => {
            *slot = None;
            window::close(id)
        }
        _ => Task::none(),
    }
}

fn subscription(state: &Hidebar) -> Subscription<Message> {
    let mut subscriptions = vec![window::close_events().map(Message::Closed)];
    let momentary = state
        .visibility
        .iter()
        .any(|visibility| matches!(visibility, Visibility::Until(_)));
    if momentary {
        subscriptions.push(iced::time::every(TICK).map(|_| Message::Tick));
    } else if state.visible(Module::Clock) {
        subscriptions.push(iced::time::every(CLOCK_TICK).map(|_| Message::Tick));
    }
    if state.pulsing() {
        subscriptions.push(iced::time::every(FRAME).map(|_| Message::Frame));
    }
    if state.calendar_surface.is_some() {
        subscriptions.push(event::listen_with(calendar_input));
    }
    // En reposo sólo queda la escucha de cierres: cero trabajo.
    Subscription::batch(subscriptions)
}

/// Teclado y foco del calendario: Esc cierra, ←/→ cambian de mes, Inicio vuelve al mes actual.
fn calendar_input(event: iced::Event, _: event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Focused) => Some(Message::Focus(id, true)),
        iced::Event::Window(window::Event::Unfocused) => Some(Message::Focus(id, false)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            ..
        }) => match key {
            Named::Escape => Some(Message::CalendarClose),
            Named::ArrowLeft => Some(Message::CalendarMonth(-1)),
            Named::ArrowRight => Some(Message::CalendarMonth(1)),
            Named::Home => Some(Message::CalendarToday),
            _ => None,
        },
        _ => None,
    }
}

/// Opacidad del pulso: 1 al inicio de cada periodo, `1 - PULSE_DIM` a la mitad, y de vuelta a 1.
fn pulse_alpha(elapsed: Duration) -> f32 {
    let phase = elapsed.as_secs_f32() % PULSE_PERIOD.as_secs_f32() / PULSE_PERIOD.as_secs_f32();
    1.0 - PULSE_DIM * (1.0 - (std::f32::consts::TAU * phase).cos()) / 2.0
}

fn severity(battery: &Battery) -> Severity {
    match battery.percent {
        _ if battery.plugged() => Severity::Normal,
        percent if percent <= BATTERY_CRITICAL => Severity::Critical,
        percent if percent <= BATTERY_LOW => Severity::Low,
        _ => Severity::Normal,
    }
}

fn now() -> Now {
    let now = chrono::Local::now();
    Now {
        time: now.format("%H:%M").to_string(),
        date: calendar::spanish_date(now.date_naive()),
        today: now.date_naive(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::BatteryStatus;

    fn input(event: Event) -> Message {
        Message::Input(Input::Event(event))
    }

    fn workspaces(active: i32) -> Message {
        input(Event::Workspaces(Workspaces {
            ids: vec![1, 2, 3],
            active,
        }))
    }

    fn battery(percent: u8, status: BatteryStatus) -> Battery {
        Battery { percent, status }
    }

    fn battery_event(percent: u8, status: BatteryStatus) -> Message {
        input(Event::Battery(battery(percent, status)))
    }

    fn show(target: Target) -> Message {
        Message::Input(Input::Command(Command::Show(target)))
    }

    fn toggle(target: Target) -> Message {
        Message::Input(Input::Command(Command::Toggle(target)))
    }

    /// Estado con lecturas de todas las fuentes.
    fn ready() -> Hidebar {
        let mut state = Hidebar::new();
        let _ = update(&mut state, workspaces(1));
        let _ = update(&mut state, battery_event(80, BatteryStatus::Discharging));
        let _ = update(&mut state, input(Event::System(System { cpu: 5, ram: 40 })));
        let _ = update(&mut state, input(Event::Network(Network::Online)));
        state
    }

    /// Hace vencer todo lo momentáneo.
    fn expire_all(state: &mut Hidebar) {
        state.expire(Instant::now() + Duration::from_secs(60));
        let _ = update(state, Message::Frame);
    }

    #[test]
    fn first_readings_do_not_show() {
        let state = ready();
        assert!(state.strip.is_none());
        assert!(MODULES.iter().all(|module| !state.visible(*module)));
    }

    #[test]
    fn switching_workspace_shows_it() {
        let mut state = ready();
        let _ = update(&mut state, workspaces(1));
        assert!(!state.visible(Module::Workspaces), "sin cambio de activo");
        let _ = update(&mut state, workspaces(2));
        assert!(state.visible(Module::Workspaces) && state.strip.is_some());
        assert!(!state.visible(Module::Clock));
    }

    #[test]
    fn show_all_is_momentary() {
        let mut state = ready();
        let _ = update(&mut state, show(Target::All));
        assert!(
            MODULES
                .iter()
                .all(|module| matches!(state.visibility(*module), Visibility::Until(_)))
        );
        expire_all(&mut state);
        assert!(state.strip.is_none());
    }

    #[test]
    fn show_needs_a_reading() {
        let mut state = Hidebar::new();
        let _ = update(&mut state, show(Target::Status));
        let _ = update(&mut state, show(Target::Workspaces));
        assert!(state.strip.is_none());
        let _ = update(&mut state, show(Target::Clock));
        assert!(state.visible(Module::Clock));
    }

    #[test]
    fn toggle_pins_and_unpins() {
        let mut state = ready();
        let _ = update(&mut state, toggle(Target::Status));
        assert_eq!(state.visibility(Module::Status), Visibility::Pinned);
        expire_all(&mut state);
        assert!(state.strip.is_some(), "fijado: no vence");
        let _ = update(&mut state, show(Target::Status));
        assert_eq!(
            state.visibility(Module::Status),
            Visibility::Pinned,
            "show no lo desfija"
        );
        let _ = update(&mut state, input(Event::Network(Network::NoInternet)));
        assert_eq!(state.visibility(Module::Status), Visibility::Pinned);
        let _ = update(&mut state, toggle(Target::Status));
        assert!(!state.visible(Module::Status) && state.strip.is_none());
    }

    #[test]
    fn toggle_all_pins_everything_then_unpins() {
        let mut state = ready();
        let _ = update(&mut state, toggle(Target::All));
        assert!(
            MODULES
                .iter()
                .all(|module| state.visibility(*module) == Visibility::Pinned)
        );
        assert!(state.calendar.is_none(), "el reloj se fija sin calendario");
        let _ = update(&mut state, toggle(Target::All));
        assert!(MODULES.iter().all(|module| !state.visible(*module)));

        // Con un solo módulo fijado, toggle all desfija en vez de fijar.
        let _ = update(&mut state, toggle(Target::Workspaces));
        let _ = update(&mut state, toggle(Target::All));
        assert!(MODULES.iter().all(|module| !state.visible(*module)));
    }

    #[test]
    fn network_and_charger_changes_show_status() {
        let mut state = ready();
        let _ = update(&mut state, input(Event::Network(Network::Online)));
        assert!(!state.visible(Module::Status), "sin cambio");
        let _ = update(&mut state, input(Event::Network(Network::Offline)));
        assert!(state.visible(Module::Status));
        expire_all(&mut state);
        let _ = update(&mut state, battery_event(79, BatteryStatus::Discharging));
        assert!(!state.visible(Module::Status), "cambio de nivel");
        let _ = update(&mut state, battery_event(79, BatteryStatus::Charging));
        assert!(state.visible(Module::Status));
    }

    #[test]
    fn muting_shows_status_but_volume_changes_do_not() {
        let audio = |muted, volume| input(Event::Audio(Audio { muted, volume }));
        let mut state = ready();
        let _ = update(&mut state, audio(false, 50));
        assert!(!state.visible(Module::Status), "primera lectura");
        let _ = update(&mut state, audio(false, 70));
        assert!(!state.visible(Module::Status), "cambio de volumen");
        let _ = update(&mut state, audio(true, 70));
        assert!(state.visible(Module::Status), "silenciado");
        expire_all(&mut state);
        let _ = update(&mut state, audio(true, 0));
        assert!(!state.visible(Module::Status), "sigue sin oírse");
        let _ = update(&mut state, audio(false, 30));
        assert!(state.visible(Module::Status), "vuelve el sonido");
    }

    #[test]
    fn battery_severity_thresholds() {
        use BatteryStatus::*;
        assert_eq!(severity(&battery(26, Discharging)), Severity::Normal);
        assert_eq!(severity(&battery(25, Discharging)), Severity::Low);
        assert_eq!(severity(&battery(10, Discharging)), Severity::Critical);
        assert_eq!(severity(&battery(5, Charging)), Severity::Normal);
        assert_eq!(severity(&battery(5, NotCharging)), Severity::Normal);
    }

    #[test]
    fn indicator_yields_only_to_status() {
        let mut state = ready();
        let _ = update(&mut state, battery_event(20, BatteryStatus::Discharging));
        assert!(state.indicator.is_some());
        let _ = update(&mut state, toggle(Target::Workspaces));
        assert!(state.indicator.is_some(), "otro módulo fijado no lo oculta");
        let _ = update(&mut state, show(Target::Status));
        assert!(
            state.indicator.is_none() && state.pulsing(),
            "status pulsa en su lugar"
        );
        expire_all(&mut state);
        assert!(state.indicator.is_some());
        let _ = update(&mut state, battery_event(20, BatteryStatus::Charging));
        expire_all(&mut state);
        assert!(state.indicator.is_none() && !state.pulsing());
    }

    #[test]
    fn pulse_fades_and_returns() {
        let at = |fraction: f32| pulse_alpha(PULSE_PERIOD.mul_f32(fraction));
        assert!((at(0.0) - 1.0).abs() < 1e-3);
        assert!((at(0.5) - (1.0 - PULSE_DIM)).abs() < 1e-3);
        assert!(at(0.25) < 1.0 && at(0.25) > 1.0 - PULSE_DIM);
        assert!((at(1.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn toggle_clock_opens_and_closes_calendar() {
        let mut state = ready();
        let _ = update(&mut state, toggle(Target::Clock));
        assert!(state.calendar.is_some() && state.calendar_surface.is_some());
        assert_eq!(state.visibility(Module::Clock), Visibility::Pinned);
        let _ = update(&mut state, Message::CalendarMonth(1));
        let shown = state.calendar.as_ref().unwrap().month;
        assert_eq!(shown, calendar::shift_month(state.now.today, 1));
        let _ = update(&mut state, Message::CalendarToday);
        assert_eq!(
            state.calendar.as_ref().unwrap().month,
            calendar::first_of_month(state.now.today)
        );
        let _ = update(&mut state, toggle(Target::Clock));
        assert!(state.calendar.is_none() && state.calendar_surface.is_none());
        assert!(!state.visible(Module::Clock), "el reloj vuelve a ocultarse");
    }

    #[test]
    fn calendar_respects_previously_pinned_clock() {
        let mut state = ready();
        let _ = update(&mut state, toggle(Target::All));
        let _ = update(&mut state, toggle(Target::Clock));
        let _ = update(&mut state, Message::CalendarClose);
        assert_eq!(state.visibility(Module::Clock), Visibility::Pinned);
    }

    #[test]
    fn calendar_closes_on_focus_loss_only_after_focus() {
        let mut state = ready();
        let _ = update(&mut state, toggle(Target::Clock));
        let surface = state.calendar_surface.unwrap();
        let _ = update(&mut state, Message::Focus(surface, false));
        assert!(state.calendar.is_some(), "nunca tuvo foco: no se cierra");
        let _ = update(&mut state, Message::Focus(surface, true));
        let _ = update(&mut state, Message::Focus(surface, false));
        assert!(state.calendar.is_none());
    }
}
