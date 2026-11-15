//! CPU y RAM: `GetSystemTimes` + `GlobalMemoryStatusEx` cada 3 s.

use std::mem::{size_of, zeroed};
use std::time::Duration;

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use crate::event::{Event, System};

const POLL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuTimes {
    total: u64,
    idle: u64,
}

pub(crate) fn listen(emit: impl Fn(Event) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let mut previous = read_cpu();
        let mut last = None;

        loop {
            std::thread::sleep(POLL);

            let current = read_cpu();
            let (Some(before), Some(after)) = (previous, current) else {
                previous = current;
                continue;
            };
            previous = current;

            let Some(ram) = read_ram() else { continue };
            let system = System {
                cpu: cpu_percent(before, after),
                ram,
            };

            if last != Some(system) {
                last = Some(system);
                if !emit(Event::System(system)) {
                    return;
                }
            }
        }
    });
}

fn read_cpu() -> Option<CpuTimes> {
    // SAFETY: FILETIME son out inicializados; la API los rellena o devuelve 0.
    let mut idle: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user: FILETIME = unsafe { zeroed() };

    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }

    let idle = to_u64(idle);
    let kernel = to_u64(kernel);
    let user = to_u64(user);

    Some(CpuTimes {
        total: kernel + user,
        idle,
    })
}

fn to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn read_ram() -> Option<u8> {
    // SAFETY: out-struct con `dwLength` correcto; la API valida el tamaño.
    let mut status: MEMORYSTATUSEX = unsafe { zeroed() };
    status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;

    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }

    Some(status.dwMemoryLoad.min(100) as u8)
}

fn cpu_percent(before: CpuTimes, after: CpuTimes) -> u8 {
    let total = after.total.saturating_sub(before.total);
    let idle = after.idle.saturating_sub(before.idle);

    if total == 0 {
        return 0;
    }

    let busy = total.saturating_sub(idle);
    ((busy * 100 + total / 2) / total).min(100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_cpu_percent() {
        let before = CpuTimes {
            total: 1000,
            idle: 800,
        };
        let after = CpuTimes {
            total: 1200,
            idle: 900,
        };

        assert_eq!(cpu_percent(before, after), 50);
        assert_eq!(cpu_percent(before, before), 0, "sin tiempo transcurrido");
        assert_eq!(cpu_percent(after, before), 0, "deltas saturantes");
    }
}
