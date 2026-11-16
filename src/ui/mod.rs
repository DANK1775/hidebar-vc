//! Vistas compartidas: dibujo de la franja y del calendario con widgets de iced.
//!
//! El shell decide qué superficie dibujar; aquí no hay tipos de Wayland, Win32 ni
//! ventanas. La misma vista se usa con wgpu (Linux) y tiny-skia (Windows).

mod view;

use iced::Element;

use crate::state::Hidebar;

/// Mensajes que produce la interacción con la vista (calendario).
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Month(i32),
    Today,
    Close,
}

/// Superficie que se está dibujando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Surface {
    Strip,
    Calendar,
}

pub(crate) fn view(state: &Hidebar, surface: Surface) -> Element<'_, Message> {
    view::view(state, surface)
}

pub(crate) use view::{CALENDAR_GAP, STRIP_HEIGHT, calendar_size};
