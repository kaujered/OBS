//! Source discovery, helper parsing and low-level readers for `VED`.

use super::*;

use ahash::AHashMap as HashMap;
use calamine::Reader;

use crate::tabular::sheet::{HeaderMap, RowSchema, open_workbook, read_sheet_rows};
use crate::tabular::wells::{row_cell, sheet_names_for_mest};
pub(super) use crate::tabular::xlsx::{cell_to_f64, cell_to_string};

#[derive(Debug)]
pub(super) struct ScannedTemplate {
    pub(super) path: PathBuf,
    pub(super) mest_code: Option<i32>,
    pub(super) mest_label: &'static str,
}

pub(super) fn scan_templates_folder(folder: &Path) -> Result<Vec<ScannedTemplate>> {
    if !folder.is_dir() {
        bail!("Папка с шаблонами не найдена: {}", folder.display());
    }

    let mut found = std::fs::read_dir(folder)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| {
                        value.eq_ignore_ascii_case("xlsm") || value.eq_ignore_ascii_case("xlsx")
                    })
                    .unwrap_or(false)
        })
        .map(|path| {
            let (mest_code, mest_label) = detect_mest(&path);
            ScannedTemplate {
                path,
                mest_code,
                mest_label,
            }
        })
        .collect::<Vec<_>>();

    found.sort_by_cached_key(|template| {
        let name = template
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_lowercase();
        (template.mest_label, name)
    });

    Ok(found)
}

pub(super) fn detect_mest(template_path: &Path) -> (Option<i32>, &'static str) {
    let name = template_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_uppercase();

    if name.contains("БНГКМ") {
        (Some(4), "БНГКМ")
    } else if name.contains("НЫДА") {
        (Some(7), "НЫДА")
    } else if name.contains("ХГКМ") {
        (Some(5), "ХГКМ")
    } else if name.contains("ЯНГКМ") {
        (Some(3), "ЯНГКМ")
    } else if (name.contains("ЮНГКМ") && name.contains("АПТ")) || name.contains("АПТ-АЛЬБ")
    {
        (Some(8), "ЮНГКМ_апт-альб")
    } else if name.contains("ЮНГКМ") {
        (Some(2), "ЮНГКМ")
    } else if name.contains("МНГКМ") || name.contains("МГПУ") {
        (Some(1), "МНГКМ")
    } else {
        (None, "Не определено")
    }
}

pub(super) fn ved_label(mest: Mest) -> &'static str {
    match mest {
        Mest::Mgpu => "МНГКМ",
        Mest::YungkmSenoman => "ЮНГКМ",
        Mest::Yangkm => "ЯНГКМ",
        Mest::Bngkm => "БНГКМ",
        Mest::Hgkm => "ХГКМ",
        Mest::MgpuNyda => "НЫДА",
        Mest::YungkmAptAlb => "ЮНГКМ_апт-альб",
    }
}

pub(super) fn report_date_from_year_month(year: i32, month: u32) -> Result<String> {
    let first_next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or_else(|| anyhow!("Некорректный год/месяц: {year}-{month:02}"))?;
    let last_day = first_next - Duration::days(1);
    Ok(last_day.format("%Y%m%d").to_string())
}

pub(super) fn parse_report_date(value: &str) -> Result<(i64, i64)> {
    let value = value.trim();
    if value.len() != 8 || !value.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("Дата отчёта должна быть в формате YYYYMMDD");
    }

    Ok((value.parse::<i64>()?, value[..6].parse::<i64>()?))
}

fn qdop_sheet_names_for_mest(mest_code: i32) -> Option<&'static [&'static str]> {
    match mest_code {
        1 => Some(sheet_names_for_mest(Mest::Mgpu)),
        2 => Some(sheet_names_for_mest(Mest::YungkmSenoman)),
        3 => Some(sheet_names_for_mest(Mest::Yangkm)),
        4 => Some(sheet_names_for_mest(Mest::Bngkm)),
        5 => Some(sheet_names_for_mest(Mest::Hgkm)),
        7 => Some(sheet_names_for_mest(Mest::MgpuNyda)),
        8 => Some(sheet_names_for_mest(Mest::YungkmAptAlb)),
        _ => None,
    }
}

pub(super) fn load_qdop_excel(
    path: &Path,
    selected_codes: &BTreeSet<i32>,
) -> Result<HashMap<i32, HashMap<i64, f64>>> {
    if !path.is_file() {
        return Ok(HashMap::new());
    }

    let mut workbook = open_workbook(path)?;
    let available_sheet_names = workbook.sheet_names().into_iter().collect::<BTreeSet<_>>();
    let mut out = HashMap::new();

    for &mest_code in selected_codes {
        let Some(target_sheets) = qdop_sheet_names_for_mest(mest_code) else {
            continue;
        };

        let mut mest_qdop = HashMap::new();
        for &sheet_name in target_sheets {
            if !available_sheet_names.contains(sheet_name) {
                continue;
            }

            let Some(rows) = read_sheet_rows(
                &mut workbook,
                sheet_name,
                RowSchema::new(0, &["well", "qdop"]),
                |headers, row| {
                    let well = row_cell(headers, row, "well").and_then(cell_to_i64)?;
                    let qdop = row_cell(headers, row, "qdop").and_then(cell_to_f64)?;
                    Some((well, qdop))
                },
            )?
            else {
                continue;
            };

            for (well, qdop) in rows {
                mest_qdop.insert(well, qdop);
            }
        }

        out.insert(mest_code, mest_qdop);
    }

    Ok(out)
}

/// Значения из режимных листов `<Месторождение>_ОПТ.xlsx` / `_ДОП.xlsx` для
/// колонок «Технологический режим» и «Допустимый режим скважин».
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RegimeRow {
    pub(super) opt_rus: Option<f64>,
    pub(super) opt_depr: Option<f64>,
    pub(super) opt_q: Option<f64>,
    pub(super) dop_rus: Option<f64>,
    pub(super) dop_depr: Option<f64>,
    pub(super) dop_q: Option<f64>,
    pub(super) dop_tus: Option<f64>,
    pub(super) dop_tgidr: Option<f64>,
    pub(super) qdop_skv: Option<f64>,
}

fn normalize_regime_name(value: &str) -> String {
    value.to_uppercase().replace('-', "_")
}

/// Имя месторождения в файлах режимных листов по имени шаблона ведомости:
/// `Ведомость_МНГКМ_ГП1` -> `МГПУ_ГП1`, `Ведомость_НЫДА` -> `МГПУ_НЫДА` и т.д.
pub(super) fn regime_key_for_template(template_path: &Path) -> Option<String> {
    let stem = template_path.file_stem()?.to_str()?;
    let mut key = normalize_regime_name(stem);
    if let Some(stripped) = key.strip_prefix("ВЕДОМОСТЬ_") {
        key = stripped.to_string();
    }
    if let Some(rest) = key.strip_prefix("МНГКМ") {
        key = format!("МГПУ{rest}");
    } else if key == "НЫДА" {
        key = "МГПУ_НЫДА".to_string();
    } else if key == "ЮНГКМ" {
        key = "ЮНГКМ_СЕНОМАН".to_string();
    }
    Some(key)
}

/// Сводная ведомость БНГКМ ГП2 собирается из листов `БНГКМ_ГП2_1` и
/// `БНГКМ_ГП2_2`, поэтому вдобавок к точному имени принимаем `<KEY>_...`.
fn regime_base_matches(base: &str, key: &str) -> bool {
    base == key
        || base
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with('_'))
}

/// Данные всех режимных листов одного шаблона ведомости.
#[derive(Debug, Clone, Default)]
pub(super) struct RegimeData {
    pub(super) rows: HashMap<i64, RegimeRow>,
    /// Рвх из строки «Рвх=» под основной таблицей (значение в колонке Ру),
    /// по одному на файл в порядке имён (для БНГКМ_ГП2: ГП2_1, затем ГП2_2).
    pub(super) opt_rvh: Vec<Option<f64>>,
    pub(super) dop_rvh: Vec<Option<f64>>,
}

pub(super) fn load_regime_for_template(
    regime_dir: &Path,
    template_path: &Path,
) -> Result<Option<RegimeData>> {
    let Some(key) = regime_key_for_template(template_path) else {
        return Ok(None);
    };

    let mut matched = Vec::new();
    for entry in std::fs::read_dir(regime_dir)
        .with_context(|| format!("Не удалось прочитать папку {}", regime_dir.display()))?
    {
        let path = entry?.path();
        let is_workbook = path.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| {
                    value.eq_ignore_ascii_case("xlsx") || value.eq_ignore_ascii_case("xlsm")
                });
        if !is_workbook {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let stem = normalize_regime_name(stem);
        let (base, is_dop) = if let Some(base) = stem.strip_suffix("_ОПТ") {
            (base, false)
        } else if let Some(base) = stem.strip_suffix("_ДОП") {
            (base, true)
        } else {
            continue;
        };
        if !regime_base_matches(base, &key) {
            continue;
        }
        matched.push((stem, path, is_dop));
    }
    if matched.is_empty() {
        return Ok(None);
    }
    matched.sort_by(|left, right| left.0.cmp(&right.0));

    let mut data = RegimeData::default();
    for (_, path, is_dop) in &matched {
        let rvh = read_regime_sheet(path, *is_dop, &mut data.rows)
            .with_context(|| format!("Не удалось прочитать режимный лист {}", path.display()))?;
        if *is_dop {
            data.dop_rvh.push(rvh);
        } else {
            data.opt_rvh.push(rvh);
        }
    }

    Ok(Some(data))
}

/// Читает лист «скважины»; возвращает Рвх из строки «Рвх=» под таблицей.
fn read_regime_sheet(
    path: &Path,
    is_dop: bool,
    out: &mut HashMap<i64, RegimeRow>,
) -> Result<Option<f64>> {
    let mut workbook = open_workbook(path)?;
    let range = workbook
        .worksheet_range("скважины")
        .map_err(|err| anyhow!("Не найден лист скважины: {err:?}"))?;
    let mut rows = range.rows();
    let Some(header_row) = rows.next() else {
        bail!("Пустой лист 'скважины'");
    };
    let headers: HeaderMap = header_row
        .iter()
        .enumerate()
        .filter_map(|(idx, cell)| cell_to_string(cell).map(|name| (name.to_lowercase(), idx)))
        .collect();
    for name in ["скважина", "ру", "депр.", "qгаз"] {
        if !headers.contains_key(name) {
            bail!("Лист 'скважины' не содержит колонку '{name}'");
        }
    }

    let mut rvh = None;
    for row in rows {
        if let Some(well) = row_cell(&headers, row, "скважина").and_then(cell_to_i64) {
            let entry = out.entry(well).or_default();
            let rus = row_cell(&headers, row, "ру").and_then(cell_to_f64);
            let depr = row_cell(&headers, row, "депр.").and_then(cell_to_f64);
            let q = row_cell(&headers, row, "qгаз").and_then(cell_to_f64);
            if is_dop {
                entry.dop_rus = rus;
                entry.dop_depr = depr;
                entry.dop_q = q;
                entry.dop_tus = row_cell(&headers, row, "ту").and_then(cell_to_f64);
                entry.dop_tgidr = row_cell(&headers, row, "тгидр").and_then(cell_to_f64);
                entry.qdop_skv = row_cell(&headers, row, "qдоп.скв.").and_then(cell_to_f64);
            } else {
                entry.opt_rus = rus;
                entry.opt_depr = depr;
                entry.opt_q = q;
            }
            continue;
        }

        let is_rvh_label = row.iter().any(|cell| {
            cell_to_string(cell).is_some_and(|value| value.trim().to_lowercase().starts_with("рвх"))
        });
        if is_rvh_label && let Some(value) = row_cell(&headers, row, "ру").and_then(cell_to_f64) {
            rvh = Some(value);
        }
    }
    Ok(rvh)
}

pub(super) fn cell_to_i64(cell: &Data) -> Option<i64> {
    crate::tabular::xlsx::cell_to_i64(cell)
        .or_else(|| cell_to_string(cell).and_then(|value| normalize_well_text(&value)))
}

pub(super) fn normalize_well_text(value: &str) -> Option<i64> {
    let mut parsed = None;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            let digit = (ch as u8 - b'0') as i64;
            let next = parsed
                .unwrap_or(0_i64)
                .checked_mul(10)?
                .checked_add(digit)?;
            parsed = Some(next);
        } else if parsed.is_some() {
            break;
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_well_text, parse_report_date, regime_base_matches, regime_key_for_template,
        report_date_from_year_month,
    };
    use std::path::Path;

    #[test]
    fn regime_key_maps_template_names_to_regime_sheet_names() {
        let key = |name: &str| regime_key_for_template(Path::new(name)).unwrap();
        assert_eq!(key("Ведомость_БНГКМ_ГП1.xlsx"), "БНГКМ_ГП1");
        assert_eq!(key("Ведомость_МНГКМ_ГП3.xlsx"), "МГПУ_ГП3");
        assert_eq!(key("Ведомость_НЫДА.xlsx"), "МГПУ_НЫДА");
        assert_eq!(key("Ведомость_ЮНГКМ.xlsx"), "ЮНГКМ_СЕНОМАН");
        assert_eq!(key("Ведомость_ЮНГКМ_апт_альб.xlsx"), "ЮНГКМ_АПТ_АЛЬБ");
    }

    #[test]
    fn regime_base_matches_exact_and_gp2_parts() {
        assert!(regime_base_matches("БНГКМ_ГП2_1", "БНГКМ_ГП2"));
        assert!(regime_base_matches("БНГКМ_ГП2_2", "БНГКМ_ГП2"));
        assert!(regime_base_matches("ХГКМ", "ХГКМ"));
        assert!(!regime_base_matches("БНГКМ_ГП1", "БНГКМ_ГП2"));
        assert!(!regime_base_matches("ЮНГКМ_АПТ_АЛЬБ", "ЮНГКМ_СЕНОМАН"));
    }

    #[test]
    fn report_date_uses_last_day_of_month() {
        assert_eq!(report_date_from_year_month(2026, 2).unwrap(), "20260228");
        assert_eq!(report_date_from_year_month(2024, 2).unwrap(), "20240229");
    }

    #[test]
    fn parse_report_date_returns_day_and_month_keys() {
        assert_eq!(parse_report_date("20260228").unwrap(), (20260228, 202602));
        assert!(parse_report_date("2026-02-28").is_err());
    }

    #[test]
    fn normalize_well_text_extracts_leading_digits_block() {
        assert_eq!(normalize_well_text("скв. 123А"), Some(123));
        assert_eq!(normalize_well_text(" 456/2 "), Some(456));
        assert_eq!(normalize_well_text("нет номера"), None);
    }
}
