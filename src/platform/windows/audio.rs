//! Audio: mute y volumen de la salida por defecto con Core Audio.
//!
//! Se re-enumera el endpoint cada 2 s: así se detectan cambios de dispositivo
//! (auriculares, HDMI) sin implementar `IMMNotificationClient`. Los cambios de
//! silencio se reflejan en como máximo ese intervalo.

use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};

use crate::event::{Audio, Event};

const POLL: Duration = Duration::from_secs(2);
/// Re-enumerar el endpoint detecta cambios de dispositivo; no hace falta cada poll.
const REFRESH: Duration = Duration::from_secs(10);

pub(crate) fn listen(emit: impl Fn(Event) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        // SAFETY: una inicialización COM por hilo; `ComGuard` equilibra el conteo.
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if initialized.is_err() {
            return;
        }
        let _guard = ComGuard;

        let mut volume: Option<IAudioEndpointVolume> = None;
        let mut refreshed = Instant::now() - REFRESH;
        let mut last = None;

        loop {
            if refreshed.elapsed() >= REFRESH {
                volume = open();
                refreshed = Instant::now();
            }

            if let Some(endpoint) = &volume {
                match read(endpoint) {
                    Some(audio) if last != Some(audio) => {
                        last = Some(audio);
                        if !emit(Event::Audio(audio)) {
                            return;
                        }
                    }
                    Some(_) => {}
                    None => volume = None,
                }
            }

            std::thread::sleep(POLL);
        }
    });
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: equilibra el `CoInitializeEx` de este mismo hilo.
        unsafe {
            CoUninitialize();
        }
    }
}

fn open() -> Option<IAudioEndpointVolume> {
    // SAFETY: COM está inicializado en este hilo; las interfaces se usan solo
    // aquí y se liberan al salir de la función.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;

        Some(volume)
    }
}

fn read(volume: &IAudioEndpointVolume) -> Option<Audio> {
    // SAFETY: la interfaz pertenece a este hilo MTA y sigue viva durante la llamada.
    unsafe {
        Some(Audio {
            muted: volume.GetMute().ok()?.as_bool(),
            volume: volume_percent(volume.GetMasterVolumeLevelScalar().ok()?),
        })
    }
}

fn volume_percent(scalar: f32) -> u16 {
    (scalar.clamp(0.0, 1.0) * 100.0).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_scalar_volume() {
        assert_eq!(volume_percent(0.0), 0);
        assert_eq!(volume_percent(0.5), 50);
        assert_eq!(volume_percent(1.0), 100);
        assert_eq!(volume_percent(1.5), 100);
    }
}
