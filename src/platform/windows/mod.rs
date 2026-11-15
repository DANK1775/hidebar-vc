//! Shell Windows: Win32 + `UpdateLayeredWindow` + `iced_tiny_skia`.
//!
//! No usa wgpu, winit ni `iced::daemon`. El bucle de mensajes es el dueño del
//! estado; el WndProc solo valida pintura y hit-testing.

mod audio;
mod battery;
mod fullscreen;
mod geometry;
mod hotkeys;
mod ipc;
mod logging;
mod network;
mod render;
mod startup;
mod system;
mod win32;

pub(crate) use startup::{install, uninstall};

use std::io;
use std::mem::zeroed;
use std::process::ExitCode;
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_HOME, VK_LEFT, VK_RIGHT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DispatchMessageW, GetMessageW, HTTRANSPARENT, MSG, PostMessageW,
    PostQuitMessage, TranslateMessage, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND,
    WM_HOTKEY, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_PAINT, WM_SETFOCUS, WM_TIMER,
};

use crate::command::Command;
use crate::event::Event;
use crate::state::{CLOCK_TICK, FRAME, Hidebar, Input, Module, TICK};
use crate::ui;
use geometry::WorkArea;
use render::LayeredSurface;

struct Shell {
    state: Hidebar,
    area: WorkArea,
    strip: LayeredSurface,
    calendar: LayeredSurface,
    strip_visible: bool,
    calendar_visible: bool,
    timer: Option<u32>,
    inputs: Receiver<Input>,
    fullscreen_rx: Receiver<bool>,
    fullscreen: bool,
    cursor: iced::mouse::Cursor,
    hotkeys: Vec<i32>,
    quit: bool,
}

impl Shell {
    fn new(
        area: WorkArea,
        strip_hwnd: HWND,
        calendar_hwnd: HWND,
        inputs: Receiver<Input>,
        fullscreen_rx: Receiver<bool>,
    ) -> Self {
        let strip = LayeredSurface::new(
            strip_hwnd,
            POINT {
                x: area.left,
                y: area.top,
            },
            area.width,
            area.strip_height(),
            area.scale,
        );

        let (x, y, width, height) = area.calendar_rect();
        let calendar =
            LayeredSurface::new(calendar_hwnd, POINT { x, y }, width, height, area.scale);

        Self {
            state: Hidebar::new(),
            area,
            strip,
            calendar,
            strip_visible: false,
            calendar_visible: false,
            timer: None,
            inputs,
            fullscreen_rx,
            fullscreen: false,
            cursor: iced::mouse::Cursor::Unavailable,
            hotkeys: hotkeys::register(strip_hwnd),
            quit: false,
        }
    }

    fn sync(&mut self) {
        if self.fullscreen {
            if self.strip_visible {
                self.strip.hide();
                self.strip_visible = false;
            }
            if self.calendar_visible {
                self.calendar.hide();
                self.calendar_visible = false;
            }
            if self.timer.is_some() {
                win32::set_timer(self.strip.window(), None);
                self.timer = None;
            }
            return;
        }

        let strip = self.state.strip_wanted();
        if strip {
            self.strip.render(
                &self.state,
                ui::Surface::Strip,
                iced::mouse::Cursor::Unavailable,
            );
            if !self.strip_visible {
                self.strip.show(false);
                self.strip_visible = true;
            }
        } else if self.strip_visible {
            self.strip.hide();
            self.strip_visible = false;
        }

        let calendar = self.state.calendar_wanted();
        if calendar {
            self.calendar
                .render(&self.state, ui::Surface::Calendar, self.cursor);
            if !self.calendar_visible {
                self.calendar.show(true);
                self.calendar_visible = true;
            }
        } else if self.calendar_visible {
            self.calendar.hide();
            self.calendar_visible = false;
        }

        self.update_timer();
    }

    fn update_timer(&mut self) {
        let interval = if self.state.pulsing() {
            Some(FRAME.as_millis() as u32)
        } else if self.state.momentary() {
            Some(TICK.as_millis() as u32)
        } else if self.state.visible(Module::Clock) {
            Some(CLOCK_TICK.as_millis() as u32)
        } else {
            None
        };

        if interval != self.timer {
            win32::set_timer(self.strip.window(), interval);
            self.timer = interval;
        }
    }

    fn on_timer(&mut self) {
        self.state.tick(Instant::now());
        self.sync();
    }

    fn on_events(&mut self) {
        while let Ok(active) = self.fullscreen_rx.try_recv() {
            self.fullscreen = active;
            if active {
                self.state.close_calendar();
            }
        }

        while let Ok(input) = self.inputs.try_recv() {
            if matches!(&input, Input::Command(Command::Quit)) {
                self.quit = true;
                return;
            }
            self.state.handle(input);
        }
        self.sync();
    }

    fn on_hotkey(&mut self, id: i32) {
        let Some(command) = hotkeys::command(id) else {
            return;
        };
        if command == Command::Quit {
            self.quit = true;
            return;
        }
        self.state.handle(Input::Command(command));
        self.sync();
    }

    fn on_mouse(&mut self, message: u32, lparam: LPARAM) {
        if !self.calendar_visible {
            return;
        }

        win32::track_mouse_leave(self.calendar.window());
        let position = iced::Point::new(
            low_word(lparam) as f32 / self.area.scale,
            high_word(lparam) as f32 / self.area.scale,
        );
        self.cursor = iced::mouse::Cursor::Available(position);

        let event = match message {
            WM_MOUSEMOVE => iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }),
            WM_LBUTTONDOWN => {
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
            }
            WM_LBUTTONUP => iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )),
            _ => return,
        };

        let messages = self.calendar.update(&self.state, event, self.cursor);
        for message in messages {
            self.apply_ui(message);
        }

        // Hover/click solo repinta el calendario; la franja no cambia.
        if self.state.calendar_wanted() {
            self.calendar
                .render(&self.state, ui::Surface::Calendar, self.cursor);
        } else {
            self.sync();
        }
    }

    fn on_mouse_leave(&mut self) {
        self.cursor = iced::mouse::Cursor::Unavailable;
        if self.calendar_visible {
            self.calendar
                .render(&self.state, ui::Surface::Calendar, self.cursor);
        }
    }

    fn on_key(&mut self, key: u32) {
        let message = match key {
            k if k == VK_ESCAPE as u32 => ui::Message::Close,
            k if k == VK_LEFT as u32 => ui::Message::Month(-1),
            k if k == VK_RIGHT as u32 => ui::Message::Month(1),
            k if k == VK_HOME as u32 => ui::Message::Today,
            _ => return,
        };

        self.apply_ui(message);
        if self.state.calendar_wanted() {
            self.calendar
                .render(&self.state, ui::Surface::Calendar, self.cursor);
        } else {
            self.sync();
        }
    }

    fn on_focus(&mut self, focused: bool) {
        self.state.mark_calendar_focused(focused);
        self.sync();
    }

    fn apply_ui(&mut self, message: ui::Message) {
        match message {
            ui::Message::Month(delta) => self.state.shift_calendar(delta),
            ui::Message::Today => self.state.calendar_today(),
            ui::Message::Close => self.state.close_calendar(),
        }
    }

    fn relayout(&mut self) {
        self.area = geometry::primary();

        self.strip.move_resize(
            POINT {
                x: self.area.left,
                y: self.area.top,
            },
            self.area.width,
            self.area.strip_height(),
            self.area.scale,
        );

        let (x, y, width, height) = self.area.calendar_rect();
        self.calendar
            .move_resize(POINT { x, y }, width, height, self.area.scale);

        self.sync();
    }

    fn run(&mut self) {
        // SAFETY: bucle de mensajes estándar del hilo UI; `message` es un out válido.
        let mut message: MSG = unsafe { zeroed() };

        while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
            match message.message {
                WM_TIMER if message.hwnd == self.strip.window() => self.on_timer(),
                win32::WM_APP_EVENT => self.on_events(),
                WM_HOTKEY => self.on_hotkey(message.wParam as i32),
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP
                    if message.hwnd == self.calendar.window() =>
                {
                    self.on_mouse(message.message, message.lParam);
                }
                WM_KEYDOWN if message.hwnd == self.calendar.window() => {
                    self.on_key(message.wParam as u32);
                }
                // WM_SETFOCUS/KILLFOCUS y WM_DPICHANGED/DISPLAYCHANGE se envían,
                // no se postean: el WndProc los reenvía con estos mensajes.
                win32::WM_APP_FOCUS => self.on_focus(message.wParam != 0),
                win32::WM_APP_RELAYOUT => self.relayout(),
                win32::WM_MOUSELEAVE if message.hwnd == self.calendar.window() => {
                    self.on_mouse_leave();
                }
                // SAFETY: reenvío estándar de los mensajes que no gestiona el shell.
                _ => unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                },
            }

            if self.quit {
                break;
            }
        }
    }
}

pub(crate) fn run_daemon() -> ExitCode {
    win32::enable_dpi_awareness();
    logging::init();
    win32::hide_console_if_own();
    let area = geometry::primary();

    let strip_class = win32::wide("hidebar_strip");
    let calendar_class = win32::wide("hidebar_calendar");
    if !win32::register_class(&strip_class, Some(strip_proc))
        || !win32::register_class(&calendar_class, Some(calendar_proc))
    {
        logging::error(format!(
            "hidebar: no se pudo registrar la clase de ventana: {}",
            io::Error::last_os_error()
        ));
        return ExitCode::FAILURE;
    }

    let title = win32::wide("hidebar");
    let strip_hwnd = win32::create_window(
        &strip_class,
        &title,
        win32::STRIP_EX_STYLE,
        area.left,
        area.top,
        area.width,
        area.strip_height(),
    );

    let (x, y, width, height) = area.calendar_rect();
    let calendar_hwnd = win32::create_window(
        &calendar_class,
        &title,
        win32::CALENDAR_EX_STYLE,
        x,
        y,
        width,
        height,
    );

    let (sender, receiver) = mpsc::channel();
    if let Err(error) = ipc::listen(sender.clone(), win32::SendHwnd(strip_hwnd)) {
        logging::error(format!("hidebar: {error}"));
        return ExitCode::FAILURE;
    }

    let hwnd = win32::SendHwnd(strip_hwnd);
    battery::listen(emitter(sender.clone(), hwnd));
    system::listen(emitter(sender.clone(), hwnd));
    network::listen(emitter(sender.clone(), hwnd));
    audio::listen(emitter(sender.clone(), hwnd));

    let (fullscreen_tx, fullscreen_rx) = mpsc::channel();
    fullscreen::listen(fullscreen_tx, hwnd);

    let mut shell = Shell::new(area, strip_hwnd, calendar_hwnd, receiver, fullscreen_rx);
    shell.sync();
    shell.run();

    hotkeys::unregister(strip_hwnd, &shell.hotkeys);

    ExitCode::SUCCESS
}

pub(crate) fn send(command: Command) -> io::Result<String> {
    ipc::send(command)
}

/// Emisor común de las fuentes: encola el evento y despierta el bucle de mensajes.
fn emitter(
    sender: mpsc::Sender<Input>,
    hwnd: win32::SendHwnd,
) -> impl Fn(Event) -> bool + Send + 'static {
    move |event| {
        if sender.send(Input::Event(event)).is_ok() {
            win32::post_event(hwnd);
            true
        } else {
            false
        }
    }
}

fn low_word(value: LPARAM) -> i32 {
    (value as u16) as i16 as i32
}

fn high_word(value: LPARAM) -> i32 {
    ((value >> 16) as u16) as i16 as i32
}

unsafe extern "system" fn strip_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { window_proc(true, hwnd, message, wparam, lparam) }
}

unsafe extern "system" fn calendar_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { window_proc(false, hwnd, message, wparam, lparam) }
}

/// WndProc de ambas ventanas. No toca el `Shell`: el bucle principal gestiona
/// timers, entrada, IPC y hotkeys fuera de la reentrada del sistema.
unsafe fn window_proc(
    is_strip: bool,
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST if is_strip => HTTRANSPARENT as LRESULT,
        WM_SETFOCUS | WM_KILLFOCUS if !is_strip => {
            let focused = usize::from(message == WM_SETFOCUS);
            // SAFETY: se reenvía el foco a la cola del hilo UI; el Shell lo
            // consume en su bucle. DefWindowProc mantiene el comportamiento base.
            unsafe {
                PostMessageW(hwnd, win32::WM_APP_FOCUS, focused, 0);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            // SAFETY: se reenvía el aviso a la cola; `relayout` recalcula la
            // geometría y recrea los DIB con el DPI nuevo.
            unsafe {
                PostMessageW(hwnd, win32::WM_APP_RELAYOUT, 0, 0);
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            // SAFETY: BeginPaint/EndPaint emparejados sobre un HWND válido.
            let mut paint: PAINTSTRUCT = unsafe { zeroed() };
            unsafe {
                BeginPaint(hwnd, &mut paint);
                EndPaint(hwnd, &paint);
            }
            0
        }
        WM_DESTROY => {
            // SAFETY: cierra el bucle de mensajes de este hilo.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: el sistema garantiza un HWND y parámetros válidos para el mensaje.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
