//! Dibujo de las superficies: pastillas de la franja, indicador y calendario.

use chrono::Datelike;
use std::sync::LazyLock;

use iced::widget::{Column, Row, button, column, container, row, space, svg, text};
use iced::{Alignment, Border, Color, Element, Length, Padding, window};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};

use super::calendar::{self, WEEKDAYS_SHORT};
use super::{Hidebar, Message, Module, NAMESPACE, Severity, severity};
use crate::event::{Audio, Battery, BatteryStatus, Network, System, Workspaces};

/// Alto de la superficie de la franja, en píxeles lógicos.
const STRIP_HEIGHT: u32 = 34;
const PILL_BACKGROUND: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);
/// Pastillas: alto, separación del borde superior y de los laterales, relleno horizontal.
const PILL_HEIGHT: f32 = 26.0;
const PILL_TOP: f32 = 6.0;
const PILL_SIDE: f32 = 12.0;
const PILL_PADDING: f32 = 12.0;
const DOT_SIZE: f32 = 7.0;

const DIM: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.6);
const WARNING_COLOR: Color = Color::from_rgb(1.0, 0.65, 0.25);
const CRITICAL_COLOR: Color = Color::from_rgb(1.0, 0.42, 0.21);
const CHARGING_COLOR: Color = Color::from_rgb(1.0, 0.82, 0.25);

/// Iconos propios (`assets/icons/`), embebidos en el binario y teñidos al dibujarlos.
const ICON_SIZE: f32 = 14.0;
static ICON_NET: LazyLock<svg::Handle> =
    LazyLock::new(|| icon(include_bytes!("../../assets/icons/net.svg")));
static ICON_NET_OFF: LazyLock<svg::Handle> =
    LazyLock::new(|| icon(include_bytes!("../../assets/icons/net-off.svg")));
static ICON_CPU: LazyLock<svg::Handle> =
    LazyLock::new(|| icon(include_bytes!("../../assets/icons/cpu.svg")));
static ICON_RAM: LazyLock<svg::Handle> =
    LazyLock::new(|| icon(include_bytes!("../../assets/icons/ram.svg")));
static ICON_MUTED: LazyLock<svg::Handle> =
    LazyLock::new(|| icon(include_bytes!("../../assets/icons/muted.svg")));

fn icon(bytes: &'static [u8]) -> svg::Handle {
    svg::Handle::from_memory(bytes)
}

/// Indicador: superficie cuadrada con un punto central.
const INDICATOR_SIZE: u32 = 24;
/// Margen (arriba, derecha, abajo, izquierda); alinea el punto con las pastillas.
const INDICATOR_MARGIN: (i32, i32, i32, i32) = (7, 12, 0, 0);
const INDICATOR_DOT: f32 = 8.0;

/// Calendario: celdas, relleno y separación bajo la franja.
const CELL: f32 = 28.0;
const CALENDAR_PADDING: f32 = 12.0;
const CALENDAR_HEADER: f32 = 28.0;
const CALENDAR_GAP: i32 = 4;
const CALENDAR_BACKGROUND: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.8);

pub fn view(state: &Hidebar, id: window::Id) -> Element<'_, Message> {
    if state.strip == Some(id) {
        strip_view(state)
    } else if state.indicator == Some(id) {
        indicator_view(fade(severity_color(state.severity()), state.pulse()))
    } else if state.calendar_surface == Some(id) {
        calendar_view(state)
    } else {
        space().into()
    }
}

/// Franja superior a lo ancho de la pantalla activa, sin reservar espacio y sin recibir entrada.
pub fn strip_settings() -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((0, STRIP_HEIGHT)),
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Left | Anchor::Right,
        // -1: no reserva espacio ni se desplaza por la zona exclusiva de otras barras.
        exclusive_zone: Some(-1),
        margin: None,
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: OutputOption::Active,
        events_transparent: true,
        namespace: Some(NAMESPACE.to_owned()),
    }
}

pub fn indicator_settings() -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((INDICATOR_SIZE, INDICATOR_SIZE)),
        anchor: Anchor::Top | Anchor::Right,
        margin: Some(INDICATOR_MARGIN),
        ..strip_settings()
    }
}

/// Debajo del reloj, centrado (anclado sólo arriba). Recibe cursor y teclado.
pub fn calendar_settings() -> NewLayerShellSettings {
    let (width, height) = calendar_size();
    NewLayerShellSettings {
        size: Some((width, height)),
        anchor: Anchor::Top,
        margin: Some((STRIP_HEIGHT as i32 + CALENDAR_GAP, 0, 0, 0)),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        events_transparent: false,
        ..strip_settings()
    }
}

fn calendar_size() -> (u32, u32) {
    let width = 7.0 * CELL + 2.0 * CALENDAR_PADDING;
    // Título, fila de días de la semana y las semanas.
    let height = CALENDAR_HEADER + CELL * (1 + calendar::WEEKS) as f32 + 2.0 * CALENDAR_PADDING;
    (width as u32, height as u32)
}

fn strip_view(state: &Hidebar) -> Element<'_, Message> {
    let module = |module: Module, content: Option<Element<'static, Message>>| match content {
        Some(content) if state.visible(module) => pill(content),
        _ => space().into(),
    };
    let left = module(
        Module::Workspaces,
        state.workspaces.as_ref().map(workspaces_view),
    );
    let center = module(Module::Clock, Some(clock_view(state)));
    let right = module(Module::Status, Some(status_view(state)));

    // Los laterales ocupan el mismo ancho, así el centro queda centrado.
    // Sólo las pastillas tienen fondo; el resto de la superficie es transparente.
    let line = row![
        container(left).width(Length::Fill),
        center,
        container(right).align_right(Length::Fill),
    ]
    .align_y(Alignment::Center);

    container(line)
        .width(Length::Fill)
        .align_top(Length::Fill)
        .padding(Padding {
            top: PILL_TOP,
            right: PILL_SIDE,
            bottom: 0.0,
            left: PILL_SIDE,
        })
        .into()
}

/// Fondo en forma de pastilla alrededor de un elemento.
fn pill(content: Element<'static, Message>) -> Element<'static, Message> {
    container(content)
        .center_y(PILL_HEIGHT)
        .padding([0.0, PILL_PADDING])
        .style(|_| container::Style {
            background: Some(PILL_BACKGROUND.into()),
            border: Border::default().rounded(PILL_HEIGHT / 2.0),
            ..Default::default()
        })
        .into()
}

fn clock_view(state: &Hidebar) -> Element<'static, Message> {
    row![
        text(state.now.time.clone()).size(16).color(Color::WHITE),
        text(state.now.date.clone()).size(11).color(DIM),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// Un punto por workspace; relleno el activo.
fn workspaces_view(workspaces: &Workspaces) -> Element<'static, Message> {
    let dots = workspaces
        .ids
        .iter()
        .map(|id| circle(DOT_SIZE, Color::WHITE, *id == workspaces.active));
    Row::with_children(dots).spacing(6).into()
}

/// red · cpu · ram · silencio · batería. Lo que todavía no tiene lectura no se muestra;
/// el silencio, sólo si no se oye nada.
fn status_view(state: &Hidebar) -> Element<'static, Message> {
    let mut line = Row::new().spacing(12).align_y(Alignment::Center);
    if let Some(network) = state.network {
        line = line.push(network_view(network));
    }
    if let Some(System { cpu, ram }) = state.system {
        line = line
            .push(metric(&ICON_CPU, cpu))
            .push(metric(&ICON_RAM, ram));
    }
    if state.audio.is_some_and(|audio: Audio| audio.silent()) {
        line = line.push(tinted(&ICON_MUTED, Color::WHITE));
    }
    if let Some(battery) = &state.battery {
        line = line.push(battery_view(battery, state.pulse()));
    }
    line.into()
}

/// Wifi blanco con internet, naranja sin salida, naranja-rojo tachado sin red.
fn network_view(network: Network) -> Element<'static, Message> {
    let (icon, color) = match network {
        Network::Online => (&ICON_NET, Color::WHITE),
        Network::NoInternet => (&ICON_NET, WARNING_COLOR),
        Network::Offline => (&ICON_NET_OFF, CRITICAL_COLOR),
    };
    tinted(icon, color)
}

/// Icono tenue + «12 %» en blanco.
fn metric(icon: &svg::Handle, percent: u8) -> Element<'static, Message> {
    row![
        tinted(icon, DIM),
        text(format!("{percent} %")).size(12).color(Color::WHITE),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

fn tinted(icon: &svg::Handle, color: Color) -> Element<'static, Message> {
    svg(icon.clone())
        .width(ICON_SIZE)
        .height(ICON_SIZE)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

/// Círculo relleno o sólo contorno.
fn circle(size: f32, color: Color, filled: bool) -> Element<'static, Message> {
    container(space())
        .width(size)
        .height(size)
        .style(move |_| container::Style {
            background: filled.then_some(color.into()),
            border: Border {
                color,
                width: 1.5,
                radius: (size / 2.0).into(),
            },
            ..Default::default()
        })
        .into()
}

/// «cargando 80 %» + icono. Mismos estados que el indicador: amarillo cargando,
/// blanco o naranja-rojo pulsando con batería baja o crítica.
fn battery_view(battery: &Battery, pulse: f32) -> Element<'static, Message> {
    const BODY_WIDTH: f32 = 20.0;
    const BODY_HEIGHT: f32 = 10.0;
    const BORDER: f32 = 1.2;
    const PADDING: f32 = 1.5;
    let inner = BODY_WIDTH - 2.0 * (BORDER + PADDING);
    let level = inner * f32::from(battery.percent) / 100.0;
    let color = battery_color(battery);
    let color = if severity(battery) == Severity::Normal {
        color
    } else {
        fade(color, pulse)
    };

    let fill = container(space())
        .width(level)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(color.into()),
            border: Border::default().rounded(1.0),
            ..Default::default()
        });
    let body = container(fill)
        .width(BODY_WIDTH)
        .height(BODY_HEIGHT)
        .padding(PADDING)
        .style(|_| container::Style {
            border: Border {
                color: Color::WHITE,
                width: BORDER,
                radius: 2.5.into(),
            },
            ..Default::default()
        });
    let nub = container(space())
        .width(2.0)
        .height(4.0)
        .style(|_| container::Style {
            background: Some(Color::WHITE.into()),
            border: Border::default().rounded(1.0),
            ..Default::default()
        });

    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    if battery.status == BatteryStatus::Charging {
        line = line.push(text("cargando").size(11).color(CHARGING_COLOR));
    }
    line.push(text(format!("{} %", battery.percent)).size(12).color(color))
        .push(row![body, nub].align_y(Alignment::Center))
        .into()
}

/// Punto del indicador, ya con el color del pulso, sobre un círculo con el fondo de las pastillas.
fn indicator_view(color: Color) -> Element<'static, Message> {
    container(circle(INDICATOR_DOT, color, true))
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(PILL_BACKGROUND.into()),
            border: Border::default().rounded(INDICATOR_SIZE as f32 / 2.0),
            ..Default::default()
        })
        .into()
}

/// ‹ mes año ›, encabezados lu–do y la grilla; hoy resaltado.
fn calendar_view(state: &Hidebar) -> Element<'static, Message> {
    let Some(open) = &state.calendar else {
        return space().into();
    };
    let month = open.month;
    let today = state.now.today;
    let is_current_month = today.year() == month.year() && today.month() == month.month();

    let title = button(
        text(calendar::month_title(month))
            .size(13)
            .width(Length::Fill)
            .center(),
    )
    .on_press(Message::CalendarToday)
    .width(Length::Fill)
    .style(flat_button);
    let header = row![
        nav_button("‹", Message::CalendarMonth(-1)),
        title,
        nav_button("›", Message::CalendarMonth(1)),
    ]
    .height(CALENDAR_HEADER)
    .align_y(Alignment::Center);

    let weekdays = Row::with_children(
        WEEKDAYS_SHORT
            .iter()
            .map(|day| cell(text(*day).size(11).color(DIM).into())),
    );

    let weeks = calendar::month_grid(month).map(|week| {
        Row::with_children(week.iter().map(|day| match day {
            Some(day) if is_current_month && *day == today.day() => cell(today_mark(*day)),
            Some(day) => cell(text(day.to_string()).size(12).color(Color::WHITE).into()),
            None => cell(space().into()),
        }))
        .into()
    });

    container(column![header, weekdays, Column::with_children(weeks)])
        .padding(CALENDAR_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(CALENDAR_BACKGROUND.into()),
            border: Border::default().rounded(14.0),
            ..Default::default()
        })
        .into()
}

fn cell(content: Element<'static, Message>) -> Element<'static, Message> {
    container(content).center(CELL).into()
}

/// Día de hoy: número oscuro sobre círculo blanco.
fn today_mark(day: u32) -> Element<'static, Message> {
    container(text(day.to_string()).size(12).color(Color::BLACK))
        .center(CELL - 4.0)
        .style(|_| container::Style {
            background: Some(Color::WHITE.into()),
            border: Border::default().rounded((CELL - 4.0) / 2.0),
            ..Default::default()
        })
        .into()
}

fn nav_button(label: &'static str, message: Message) -> Element<'static, Message> {
    button(text(label).size(16).center())
        .on_press(message)
        .width(CELL)
        .style(flat_button)
        .into()
}

/// Botón sin fondo; un leve resalte al pasar el cursor.
fn flat_button(_: &iced::Theme, status: button::Status) -> button::Style {
    let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hover.then_some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into()),
        text_color: Color::WHITE,
        border: Border::default().rounded(8.0),
        ..Default::default()
    }
}

/// Color de la batería: amarillo cargando; si no, según severidad.
fn battery_color(battery: &Battery) -> Color {
    if battery.status == BatteryStatus::Charging {
        CHARGING_COLOR
    } else {
        severity_color(severity(battery))
    }
}

fn severity_color(severity: Severity) -> Color {
    match severity {
        Severity::Critical => CRITICAL_COLOR,
        Severity::Normal | Severity::Low => Color::WHITE,
    }
}

fn fade(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha,
        ..color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(percent: u8, status: BatteryStatus) -> Battery {
        Battery { percent, status }
    }

    #[test]
    fn charging_is_yellow() {
        use BatteryStatus::*;
        assert_eq!(battery_color(&battery(50, Charging)), CHARGING_COLOR);
        assert_eq!(battery_color(&battery(5, Charging)), CHARGING_COLOR);
        assert_eq!(battery_color(&battery(50, Full)), Color::WHITE);
        assert_eq!(battery_color(&battery(5, Discharging)), CRITICAL_COLOR);
    }

    #[test]
    fn calendar_fits_six_weeks() {
        let (width, height) = calendar_size();
        assert_eq!(width, 220);
        assert_eq!(height, 28 + 28 * 7 + 24);
    }
}
