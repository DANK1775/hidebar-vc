//! Fuente de red: ruta por defecto (`/proc/net/route`) y, si la hay, una prueba de salida
//! por TCP a `1.1.1.1:443` (sin enviar datos). Decisión en ADR-0002, Q1.

use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::event::{Event, Network};

const PROBE_TARGET: SocketAddr = SocketAddr::V4(std::net::SocketAddrV4::new(
    std::net::Ipv4Addr::new(1, 1, 1, 1),
    443,
));
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const ROUTE_POLL: Duration = Duration::from_secs(5);
const PROBE_EVERY: Duration = Duration::from_secs(30);

/// Vigila la red en un hilo propio. Emite el estado inicial y luego sólo los cambios.
/// `emit` devuelve `false` cuando el receptor ya no existe.
pub fn listen(emit: impl Fn(Event) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let mut last: Option<Network> = None;
        let mut had_route = false;
        let mut last_probe: Option<(Instant, bool)> = None;
        loop {
            let has_route = fs::read_to_string("/proc/net/route")
                .is_ok_and(|routes| has_default_route(&routes));
            let network = if has_route {
                // Se prueba al aparecer la ruta y luego cada PROBE_EVERY.
                let due =
                    !had_route || last_probe.is_none_or(|(at, _)| at.elapsed() >= PROBE_EVERY);
                if due {
                    last_probe = Some((Instant::now(), probe()));
                }
                match last_probe {
                    Some((_, true)) => Network::Online,
                    _ => Network::NoInternet,
                }
            } else {
                last_probe = None;
                Network::Offline
            };
            had_route = has_route;
            if last != Some(network) {
                last = Some(network);
                if !emit(Event::Network(network)) {
                    return;
                }
            }
            std::thread::sleep(ROUTE_POLL);
        }
    });
}

fn probe() -> bool {
    TcpStream::connect_timeout(&PROBE_TARGET, PROBE_TIMEOUT).is_ok()
}

/// Hay una ruta con destino 0.0.0.0 y la bandera RTF_UP (0x1).
fn has_default_route(routes: &str) -> bool {
    routes.lines().skip(1).any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        matches!(fields.as_slice(), [_, "00000000", _, flags, ..]
            if u16::from_str_radix(flags, 16).is_ok_and(|flags| flags & 0x1 != 0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n";

    #[test]
    fn detects_default_route() {
        let routes = format!(
            "{HEADER}wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
             wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n"
        );
        assert!(has_default_route(&routes));
    }

    #[test]
    fn ignores_local_and_down_routes() {
        let local =
            format!("{HEADER}wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n");
        assert!(!has_default_route(&local));
        let down =
            format!("{HEADER}wlan0\t00000000\t0101A8C0\t0002\t0\t0\t600\t00000000\t0\t0\t0\n");
        assert!(!has_default_route(&down));
        assert!(!has_default_route(HEADER));
    }

    /// `cargo test -- --ignored`: lee la tabla de rutas real y hace una prueba TCP.
    #[test]
    #[ignore = "usa la red del equipo"]
    fn checks_real_network() {
        let route = has_default_route(&fs::read_to_string("/proc/net/route").unwrap());
        println!("ruta por defecto: {route}, salida: {}", route && probe());
    }
}
