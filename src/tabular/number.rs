//! Разбор чисел из текстовых ячеек DBF и XLSX.

/// Число из ячейки, где десятичный разделитель может быть запятой.
pub(crate) fn parse_loose_f64(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    (!trimmed.is_empty())
        .then_some(trimmed.replace(',', "."))
        .and_then(|value| value.parse::<f64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_and_dot_separators() {
        assert_eq!(parse_loose_f64(" 12,5 "), Some(12.5));
        assert_eq!(parse_loose_f64("12.5"), Some(12.5));
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert_eq!(parse_loose_f64("   "), None);
        assert_eq!(parse_loose_f64("нет"), None);
    }
}
