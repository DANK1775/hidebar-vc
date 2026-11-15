//! Eventos que emiten las fuentes. Llevan hechos, no presentación:
//! la interfaz decide si se muestran, cómo y por cuánto tiempo.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Sin fuente de workspaces en Windows v1; se conserva para Linux.
    #[cfg_attr(windows, allow(dead_code))]
    Workspaces(Workspaces),
    Battery(Battery),
    System(System),
    Network(Network),
    Audio(Audio),
}

/// Salida de audio por defecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audio {
    pub muted: bool,
    /// Volumen del primer canal, en porcentaje (puede superar 100).
    pub volume: u16,
}

impl Audio {
    /// No se oye nada: silenciado o a 0 %.
    pub fn silent(&self) -> bool {
        self.muted || self.volume == 0
    }
}

/// Uso de CPU y RAM, en porcentaje.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct System {
    pub cpu: u8,
    pub ram: u8,
}

/// Conectividad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// Hay salida a internet.
    Online,
    /// Hay ruta por defecto (red local), pero la prueba de salida falla.
    NoInternet,
    /// Sin ruta por defecto.
    Offline,
}

/// Workspaces normales existentes, ordenados por id, y el activo del monitor enfocado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspaces {
    pub ids: Vec<i32>,
    pub active: i32,
}

/// Estado de la batería tal como lo informa el sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub status: BatteryStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    /// Conectado, pero sin cargar (p. ej. por límite de carga).
    NotCharging,
    Unknown,
}

impl Battery {
    /// Conectado al cargador. `Unknown` cuenta como conectado para no alarmar sin datos.
    pub fn plugged(&self) -> bool {
        self.status != BatteryStatus::Discharging
    }
}
