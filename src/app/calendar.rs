//! Fechas en español y la grilla del calendario. Sin depender de los locales del sistema.

use chrono::{Datelike, Months, NaiveDate};

const WEEKDAYS: [&str; 7] = [
    "lunes",
    "martes",
    "miércoles",
    "jueves",
    "viernes",
    "sábado",
    "domingo",
];
/// Encabezados de columna, lunes primero.
pub const WEEKDAYS_SHORT: [&str; 7] = ["lu", "ma", "mi", "ju", "vi", "sá", "do"];
const MONTHS: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

/// Semanas que muestra siempre la grilla, para que el panel no cambie de alto entre meses.
pub const WEEKS: usize = 6;

/// «jueves 12 de noviembre».
pub fn spanish_date(date: NaiveDate) -> String {
    let weekday = WEEKDAYS[date.weekday().num_days_from_monday() as usize];
    format!("{weekday} {} de {}", date.day(), month_name(date))
}

/// «noviembre 2026».
pub fn month_title(month: NaiveDate) -> String {
    format!("{} {}", month_name(month), month.year())
}

fn month_name(date: NaiveDate) -> &'static str {
    MONTHS[date.month0() as usize]
}

/// Primer día del mes de `date`.
pub fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("el día 1 siempre existe")
}

/// Primer día del mes desplazado `delta` meses.
pub fn shift_month(month: NaiveDate, delta: i32) -> NaiveDate {
    let month = first_of_month(month);
    let months = Months::new(delta.unsigned_abs());
    let shifted = if delta >= 0 {
        month.checked_add_months(months)
    } else {
        month.checked_sub_months(months)
    };
    shifted.unwrap_or(month)
}

/// Grilla lunes–domingo de `WEEKS` semanas; `None` en las celdas fuera del mes.
pub fn month_grid(month: NaiveDate) -> [[Option<u32>; 7]; WEEKS] {
    let first = first_of_month(month);
    let offset = first.weekday().num_days_from_monday() as usize;
    let days = shift_month(first, 1)
        .pred_opt()
        .map_or(28, |last| last.day()) as usize;
    let mut grid = [[None; 7]; WEEKS];
    for day in 0..days {
        let cell = offset + day;
        if let Some(week) = grid.get_mut(cell / 7) {
            week[cell % 7] = Some(day as u32 + 1);
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn formats_spanish_dates() {
        assert_eq!(spanish_date(date(2026, 11, 12)), "jueves 12 de noviembre");
        assert_eq!(spanish_date(date(2026, 3, 1)), "domingo 1 de marzo");
        assert_eq!(
            spanish_date(date(2025, 12, 31)),
            "miércoles 31 de diciembre"
        );
        assert_eq!(month_title(date(2026, 11, 12)), "noviembre 2026");
    }

    #[test]
    fn shifts_months_across_years() {
        assert_eq!(shift_month(date(2026, 11, 30), 1), date(2026, 12, 1));
        assert_eq!(shift_month(date(2026, 12, 15), 1), date(2027, 1, 1));
        assert_eq!(shift_month(date(2026, 1, 31), -1), date(2025, 12, 1));
    }

    #[test]
    fn builds_month_grid() {
        // Noviembre 2026 empieza en domingo y tiene 30 días.
        let grid = month_grid(date(2026, 11, 12));
        assert_eq!(grid[0], [None, None, None, None, None, None, Some(1)]);
        assert_eq!(grid[1][0], Some(2));
        assert_eq!(grid[5][0], Some(30));
        assert_eq!(grid[5][1], None);
        // Febrero 2027 empieza en lunes, 28 días: cuatro semanas completas y dos vacías.
        let grid = month_grid(date(2027, 2, 1));
        assert_eq!(grid[0][0], Some(1));
        assert_eq!(grid[3][6], Some(28));
        assert!(grid[4].iter().all(Option::is_none));
        // Febrero bisiesto.
        let days: usize = month_grid(date(2028, 2, 1))
            .iter()
            .flatten()
            .filter(|cell| cell.is_some())
            .count();
        assert_eq!(days, 29);
    }
}
