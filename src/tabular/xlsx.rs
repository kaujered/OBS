//! Shared `calamine::Data` converters for workbook cells.

use std::sync::Arc;

use calamine::Data;

use super::number::parse_loose_f64;

pub(crate) fn cell_to_display_string(cell: &Data) -> String {
    match cell {
        Data::DateTime(value) if value.is_datetime() => {
            let (year, month, day, hour, minute, second, milli) = value.to_ymd_hms_milli();
            if hour == 0 && minute == 0 && second == 0 && milli == 0 {
                format!("{year:04}-{month:02}-{day:02}")
            } else {
                format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
            }
        }
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(value) | Data::DateTimeIso(value) | Data::DurationIso(value) => {
            value.trim().to_string()
        }
        Data::Float(value) => {
            if value.fract().abs() < 1e-9 {
                format!("{value:.0}")
            } else {
                value.to_string()
            }
        }
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::DateTime(value) => value.to_string(),
    }
}

pub(crate) fn cell_to_string(cell: &Data) -> Option<String> {
    let text = cell_to_display_string(cell);
    (!text.trim().is_empty()).then_some(text)
}

pub(crate) fn cell_to_shared_string(cell: &Data) -> Option<Arc<str>> {
    cell_to_string(cell).map(Arc::<str>::from)
}

pub(crate) fn cell_to_f64(cell: &Data) -> Option<f64> {
    match cell {
        Data::Float(value) => Some(*value),
        Data::Int(value) => Some(*value as f64),
        _ => cell_to_string(cell).and_then(|value| parse_loose_f64(&value)),
    }
}

pub(crate) fn cell_to_i64(cell: &Data) -> Option<i64> {
    match cell {
        Data::Int(value) => Some(*value),
        Data::Float(value) => Some(value.round() as i64),
        _ => cell_to_string(cell).and_then(|value| value.parse::<i64>().ok()),
    }
}
