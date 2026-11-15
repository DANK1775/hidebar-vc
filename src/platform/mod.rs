//! Selección de plataforma. `main` solo conoce `run_daemon` y `send`.

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(windows)]
pub(crate) mod windows;

#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("hidebar solo soporta Linux y Windows");

#[cfg(target_os = "linux")]
pub(crate) use linux::{install, run_daemon, send, uninstall};
#[cfg(windows)]
pub(crate) use windows::{install, run_daemon, send, uninstall};
