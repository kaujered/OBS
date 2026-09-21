//! Shared row and table models for the KOTS branch.

use std::{borrow::Cow, sync::Arc};

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use anyhow::{Result, anyhow, bail};
use chrono::{Datelike, NaiveDate};

use super::{DIGITS_RE, WELL_SUFFIX_RE};
use crate::modes::gdi::limits::{JonesInput, JonesResult, LimitInput, LimitResult, LimitTarget};
use crate::tabular::sheet::TextSheet as SheetTable;

#[derive(Clone, Default)]
pub(super) struct WellRow {
    pub(super) plast: Option<Arc<str>>,
    pub(super) gp: Option<Arc<str>>,
    pub(super) depr: Option<f64>,
    pub(super) tpl: Option<f64>,
    pub(super) gauge: Option<f64>,
}

#[derive(Clone)]
pub(super) struct OutputRow {
    pub(super) gp: Option<Arc<str>>,
    pub(super) plast: Option<Arc<str>>,
    pub(super) regime_no: Option<String>,
    pub(super) washer: Option<f64>,
    pub(super) well: i64,
    pub(super) date_key: i64,
    pub(super) thp: Option<f64>,
    pub(super) flo: Option<f64>,
    pub(super) gauge: Option<f64>,
    pub(super) bhp: Option<f64>,
    pub(super) pst: Option<f64>,
    pub(super) ppl: Option<f64>,
    pub(super) tpl: Option<f64>,
    pub(super) water: Option<f64>,
    pub(super) sand: Option<f64>,
    pub(super) dep: Option<f64>,
    pub(super) max_dep: Option<f64>,
    pub(super) c: Option<f64>,
    pub(super) n: Option<f64>,
    pub(super) max_flo: Option<f64>,
    pub(super) dop_skv_flo: Option<f64>,
    pub(super) dop_skv_flo_percent_5: Option<f64>,
    pub(super) dop_skv_flo_095: Option<f64>,
    pub(super) limit_comment: Option<String>,
    pub(super) jones_a: Option<f64>,
    pub(super) jones_b: Option<f64>,
}

pub(super) fn parse_bngkm_table(
    table: &SheetTable,
    year: i32,
    osv: bool,
    gp3: bool,
    wells: &HashMap<i64, WellRow>,
) -> Result<Vec<OutputRow>> {
    let date_idx = find_header_exact(table, if gp3 { "Дата" } else { "Дата ГДИ" })?;
    let well_idx = find_header_exact(table, if gp3 { "Скв" } else { "№ скв" })?;
    let regime_idx = find_header_exact(table, "№ режима")?;
    let thp_idx = find_header_contains(
        table,
        if gp3 {
            "рб, ати"
        } else {
            "рбуф (ата)"
        },
    )?;
    let flo_idx = if gp3 {
        find_header_contains(table, "qдикт, тыс. м3/сут")?
    } else if !osv && year >= 2026 {
        // From 2026 onward the flow column was renamed; use "Q, тыс. м3/сут" instead.
        find_header_contains(table, "Q, тыс. м3/сут")?
    } else if year >= 2021 || osv {
        find_header_contains(table, "дебит газа по дикт")?
    } else {
        find_header_contains(table, "qm расчётный по метран")?
    };
    let bhp_idx = find_header_contains(
        table,
        if gp3 {
            "рпл, ати"
        } else {
            "р заб, кгс/см2 (ата)"
        },
    )?;
    let bhp2_idx = gp3
        .then(|| find_header_contains(table, "рпл, ата"))
        .transpose()?;
    let sand_idx = find_header_contains(
        table,
        if gp3 {
            "vуд п, мм3/м3"
        } else {
            "уд. сод. мех. примесей"
        },
    )?;
    let water_idx = find_header_contains(
        table,
        if gp3 {
            "vуд.ж, см3/м3"
        } else {
            "уд. сод. жидкости в газе"
        },
    )?;
    let washer_idx = find_header_contains(
        table,
        if gp3 {
            "шайба"
        } else {
            "диаметр диафрагмы"
        },
    )?;
    let cn_idx = resolve_cn_indexes(table, year, osv, gp3)?;
    let data_row_offset = usize::from(has_secondary_header_row(table));
    let cn_map = build_cn_map(table, well_idx, cn_idx, data_row_offset);

    let mut rows = Vec::new();
    for row in table.rows.iter().skip(data_row_offset) {
        let Some(well) = normalize_well(row.get(well_idx).map(String::as_str).unwrap_or_default())
        else {
            continue;
        };
        let Some(date_key) =
            parse_date_key(row.get(date_idx).map(String::as_str).unwrap_or_default())
        else {
            continue;
        };
        let meta = wells.get(&well);
        let bhp = if gp3 {
            match parse_num(get_value(row, bhp_idx)) {
                Some(value) => Some(value + 1.01325),
                None => bhp2_idx.and_then(|idx| parse_num(get_value(row, idx))),
            }
        } else {
            parse_num(get_value(row, bhp_idx))
        };
        let thp = parse_num(get_value(row, thp_idx))
            .map(|value| if gp3 { value + 1.01325 } else { value });
        let (c, n) = cn_map.get(&well).copied().unwrap_or((None, None));
        rows.push(OutputRow {
            gp: meta.and_then(|v| v.gp.clone()),
            plast: meta.and_then(|v| v.plast.clone()),
            regime_no: clean_text(row.get(regime_idx)),
            washer: parse_num(get_value(row, washer_idx)),
            well,
            date_key,
            thp: thp.map(|value| round_to(value, 2)),
            flo: parse_num(get_value(row, flo_idx)).map(|value| round_to(value * 1000.0, 0)),
            gauge: meta.and_then(|v| v.gauge),
            bhp: bhp.map(|value| round_to(value, 2)),
            pst: None,
            ppl: None,
            tpl: meta.and_then(|v| v.tpl),
            water: parse_num(get_value(row, water_idx)).map(|value| round_to(value, 1)),
            sand: parse_num(get_value(row, sand_idx)).map(|value| round_to(value, 1)),
            dep: Some(0.0),
            max_dep: meta.and_then(|v| v.depr),
            c,
            n,
            max_flo: None,
            dop_skv_flo: None,
            dop_skv_flo_percent_5: None,
            dop_skv_flo_095: None,
            limit_comment: None,
            jones_a: None,
            jones_b: None,
        });
    }
    assign_pressures(&mut rows, true);
    rows.retain(|row| !is_rejected(row.regime_no.as_deref()));
    Ok(rows)
}

pub(super) fn parse_hgkm_table(
    table: &SheetTable,
    wells: &HashMap<i64, WellRow>,
) -> Result<Vec<OutputRow>> {
    let date_idx = find_header_exact(table, "Дата ГДИ")?;
    let well_idx = find_header_exact(table, "№ скв")?;
    let regime_idx = find_header_exact(table, "№ режима")?;
    let thp_idx = find_header_contains(table, "рбуф (ата)")?;
    let flo_idx = find_header_contains(table, "дебит газа по дикт")?;
    let bhp_idx = find_header_contains(table, "р заб, кгс/см2 (ата)")?;
    let sand_idx = find_header_contains(table, "уд. сод. мех. примесей")?;
    let water_idx = find_header_contains(table, "уд. сод. жидкости в газе")?;
    let washer_idx = find_header_contains(table, "диаметр диафрагмы")?;
    let (c_idx, n_idx) = resolve_cn_indexes(table, 0, false, false)?;
    let data_row_offset = usize::from(has_secondary_header_row(table));
    let cn_map = build_cn_map(table, well_idx, (c_idx, n_idx), data_row_offset);

    let mut rows = Vec::new();
    for row in table.rows.iter().skip(data_row_offset) {
        let Some(well) = normalize_well(row.get(well_idx).map(String::as_str).unwrap_or_default())
        else {
            continue;
        };
        let Some(date_key) = parse_date_key(get_value(row, date_idx)) else {
            continue;
        };
        let thp = parse_num(get_value(row, thp_idx));
        let bhp = parse_num(get_value(row, bhp_idx));
        let meta = wells.get(&well);
        let (c, n) = cn_map.get(&well).copied().unwrap_or((None, None));
        rows.push(OutputRow {
            gp: meta.and_then(|v| v.gp.clone()),
            plast: meta.and_then(|v| v.plast.clone()),
            regime_no: clean_text(row.get(regime_idx)),
            washer: parse_num(get_value(row, washer_idx)),
            well,
            date_key,
            thp: thp.map(|value| round_to(value, 2)),
            flo: parse_num(get_value(row, flo_idx)).map(|value| round_to(value * 1000.0, 0)),
            gauge: meta.and_then(|v| v.gauge),
            bhp: bhp.map(|value| round_to(value, 2)),
            pst: None,
            ppl: None,
            tpl: meta.and_then(|v| v.tpl),
            water: parse_num(get_value(row, water_idx)).map(|value| round_to(value, 1)),
            sand: parse_num(get_value(row, sand_idx)).map(|value| round_to(value, 1)),
            dep: Some(0.0),
            max_dep: meta.and_then(|v| v.depr),
            c,
            n,
            max_flo: None,
            dop_skv_flo: None,
            dop_skv_flo_percent_5: None,
            dop_skv_flo_095: None,
            limit_comment: None,
            jones_a: None,
            jones_b: None,
        });
    }
    assign_pressures(&mut rows, false);
    rows.retain(|row| !is_rejected(row.regime_no.as_deref()));
    Ok(rows)
}

impl LimitTarget for OutputRow {
    fn group_key(&self) -> (i64, i64) {
        (self.well, self.date_key)
    }

    fn limit_input(&self) -> LimitInput {
        LimitInput {
            regime_no: self.regime_no.as_deref().and_then(parse_num),
            flo: self.flo,
            sand: self.sand,
            dep: self.dep,
            max_dep: self.max_dep,
        }
    }

    fn jones_input(&self) -> JonesInput {
        JonesInput {
            flo: self.flo,
            thp: self.thp,
            ppl: self.ppl,
            bhp: self.bhp,
        }
    }

    fn apply(&mut self, limits: &LimitResult, jones: &JonesResult) {
        self.max_flo = limits.max_flo;
        self.dop_skv_flo = limits.dop_skv_flo;
        self.dop_skv_flo_percent_5 = limits.dop_skv_flo_percent_5;
        self.dop_skv_flo_095 = limits.dop_skv_flo_095;
        self.limit_comment = Some(limits.comment.clone());
        self.jones_a = jones.a;
        self.jones_b = jones.b;
    }
}

fn find_header_exact(table: &SheetTable, name: &str) -> Result<usize> {
    let needle = lowercase_fast_path(name);
    table
        .headers_lc
        .iter()
        .position(|value| value == needle.as_ref())
        .ok_or_else(|| anyhow!("Не найдена колонка {name}"))
}

fn find_header_contains(table: &SheetTable, needle: &str) -> Result<usize> {
    let needle_lc = lowercase_fast_path(needle);
    let mut found = None;
    for (index, value) in table.headers_lc.iter().enumerate() {
        if !value.contains(needle_lc.as_ref()) {
            continue;
        }
        if found.replace(index).is_some() {
            bail!("Найдено несколько колонок: {needle}");
        }
    }
    found.ok_or_else(|| anyhow!("Не найдена колонка: {needle}"))
}

#[inline]
fn lowercase_fast_path(value: &str) -> Cow<'_, str> {
    if value.chars().any(char::is_uppercase) {
        Cow::Owned(value.to_lowercase())
    } else {
        Cow::Borrowed(value)
    }
}

fn has_secondary_header_row(table: &SheetTable) -> bool {
    table.rows.first().is_some_and(|row| {
        row.iter().any(|value| cn_label(value) == Some('c'))
            && row.iter().any(|value| cn_label(value) == Some('n'))
    })
}

fn resolve_cn_indexes(
    table: &SheetTable,
    year: i32,
    osv: bool,
    gp3: bool,
) -> Result<(usize, usize)> {
    if gp3 {
        return gp3_cn_indexes(table);
    }
    if let Some(indexes) = cn_indexes_from_secondary_header(table) {
        return Ok(indexes);
    }
    legacy_cn_indexes(table, year, osv)
}

fn cn_indexes_from_secondary_header(table: &SheetTable) -> Option<(usize, usize)> {
    let row = table.rows.first()?;
    if !has_secondary_header_row(table) {
        return None;
    }
    for idx in 0..row.len().saturating_sub(1) {
        if cn_label(row.get(idx).map(String::as_str).unwrap_or_default()) != Some('c') {
            continue;
        }
        if cn_label(row.get(idx + 1).map(String::as_str).unwrap_or_default()) != Some('n') {
            continue;
        }
        let section_start = idx.saturating_sub(2);
        let section_end = idx.min(table.headers_lc.len().saturating_sub(1));
        if (section_start..=section_end).any(|header_idx| {
            table
                .headers_lc
                .get(header_idx)
                .is_some_and(|value| looks_like_cn_section(value))
        }) {
            return Some((idx, idx + 1));
        }
    }
    None
}

fn gp3_cn_indexes(table: &SheetTable) -> Result<(usize, usize)> {
    for idx in 0..table.headers.len().saturating_sub(1) {
        if cn_label(
            table
                .headers
                .get(idx)
                .map(String::as_str)
                .unwrap_or_default(),
        ) == Some('c')
            && cn_label(
                table
                    .headers
                    .get(idx + 1)
                    .map(String::as_str)
                    .unwrap_or_default(),
            ) == Some('n')
        {
            return Ok((idx, idx + 1));
        }
    }
    bail!("Не найдены колонки C/N для GP-3")
}

fn build_cn_map(
    table: &SheetTable,
    well_idx: usize,
    cn_idx: (usize, usize),
    data_row_offset: usize,
) -> HashMap<i64, (Option<f64>, Option<f64>)> {
    let (c_idx, n_idx) = cn_idx;
    let mut out: HashMap<i64, (Option<f64>, Option<f64>)> = HashMap::new();
    for row in table.rows.iter().skip(data_row_offset) {
        let Some(well) = normalize_well(row.get(well_idx).map(String::as_str).unwrap_or_default())
        else {
            continue;
        };
        let c = parse_num(get_value(row, c_idx)).map(|value| round_to(value, 6));
        let n = parse_num(get_value(row, n_idx)).map(|value| round_to(value, 6));
        if c.is_none() && n.is_none() {
            continue;
        }
        let next_score = usize::from(c.is_some()) + usize::from(n.is_some());
        match out.get(&well).copied() {
            Some((prev_c, prev_n)) => {
                let prev_score = usize::from(prev_c.is_some()) + usize::from(prev_n.is_some());
                if next_score > prev_score {
                    out.insert(well, (c, n));
                }
            }
            None => {
                out.insert(well, (c, n));
            }
        }
    }
    out
}

fn cn_label(value: &str) -> Option<char> {
    match value.trim() {
        "C" | "c" | "С" | "с" => Some('c'),
        "N" | "n" | "Н" | "н" => Some('n'),
        _ => None,
    }
}

fn looks_like_cn_section(value: &str) -> bool {
    (value.contains("коэфф") || value.contains("расчет"))
        && (value.contains('c') || value.contains("с"))
        && value.contains('n')
}

fn legacy_cn_indexes(table: &SheetTable, year: i32, osv: bool) -> Result<(usize, usize)> {
    if osv {
        Ok((
            find_header_exact(table, "Unnamed: 44")?,
            find_header_exact(table, "Unnamed: 45")?,
        ))
    } else if year >= 2022 {
        Ok((
            find_header_exact(table, "Unnamed: 57")?,
            find_header_exact(table, "Unnamed: 58")?,
        ))
    } else if year == 2021 {
        Ok((
            find_header_exact(table, "Unnamed: 56")?,
            find_header_exact(table, "Unnamed: 57")?,
        ))
    } else if year == 2020 {
        Ok((
            find_header_exact(table, "Unnamed: 53")?,
            find_header_exact(table, "Unnamed: 54")?,
        ))
    } else {
        Ok((
            find_header_exact(table, "Unnamed: 51")?,
            find_header_exact(table, "Unnamed: 52")?,
        ))
    }
}

fn get_value(row: &[String], idx: usize) -> &str {
    row.get(idx).map(String::as_str).unwrap_or_default()
}

fn clean_text(value: Option<&String>) -> Option<String> {
    value
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn parse_num(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.contains(',') {
        value.replace(',', ".").parse::<f64>().ok()
    } else {
        value.parse::<f64>().ok()
    }
}

fn round_to(value: f64, digits: i32) -> f64 {
    let factor = 10_f64.powi(digits);
    (value * factor).round() / factor
}

pub(super) fn compare_regime(left: Option<&str>, right: Option<&str>) -> std::cmp::Ordering {
    match (left.and_then(parse_num), right.and_then(parse_num)) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        _ => left.unwrap_or_default().cmp(right.unwrap_or_default()),
    }
}

fn is_rejected(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let text = lowercase_fast_path(value);
    text.contains("ст до") || text.contains("ст после")
}

fn assign_pressures(rows: &mut [OutputRow], dedup_regimes: bool) {
    let mut by_well = HashMap::<i64, Vec<usize>>::new();
    for (idx, row) in rows.iter().enumerate() {
        by_well.entry(row.well).or_default().push(idx);
    }

    for indexes in by_well.values() {
        let filtered = if dedup_regimes {
            let mut last_by_regime = HashMap::<&str, usize>::new();
            for &idx in indexes {
                last_by_regime.insert(rows[idx].regime_no.as_deref().unwrap_or_default(), idx);
            }

            let mut kept = Vec::with_capacity(indexes.len());
            for &idx in indexes {
                let key = rows[idx].regime_no.as_deref().unwrap_or_default();
                if last_by_regime.get(key).copied() == Some(idx) {
                    kept.push(idx);
                }
            }
            kept.sort_unstable();
            kept
        } else {
            indexes.clone()
        };

        let ppl = filtered.iter().find_map(|idx| rows[*idx].bhp);
        let pst = filtered
            .iter()
            .filter_map(|idx| rows[*idx].thp)
            .fold(None, |acc, value| {
                Some(acc.map_or(value, |best: f64| best.max(value)))
            });

        for &idx in indexes {
            rows[idx].ppl = ppl.map(|value| round_to(value, 2));
            rows[idx].pst = pst.map(|value| round_to(value, 2));
            rows[idx].dep = match (rows[idx].ppl, rows[idx].bhp) {
                (Some(ppl), Some(bhp)) => Some(round_to(ppl - bhp, 1)),
                _ => Some(0.0),
            };
        }
    }
}

pub(super) fn dedup_rows(rows: &mut Vec<OutputRow>) {
    let mut seen = HashSet::<(
        i64,
        i64,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    )>::new();
    rows.retain(|row| {
        seen.insert((
            row.well,
            row.date_key,
            scaled_num_key(row.thp),
            scaled_num_key(row.bhp),
            scaled_num_key(row.washer),
            row.regime_no.clone(),
        ))
    });
}

fn scaled_num_key(value: Option<f64>) -> Option<i64> {
    value.map(|value| (value * 100_000_000.0).round() as i64)
}

pub(super) fn parse_date_key(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    for pattern in [
        "%d.%m.%Y",
        "%Y-%m-%d",
        "%Y-%m-%d %H:%M:%S",
        "%d.%m.%Y %H:%M:%S",
        "%Y/%m/%d",
    ] {
        if let Ok(date) = NaiveDate::parse_from_str(value, pattern) {
            return Some(date_to_key(date));
        }
    }

    if value.len() >= 10 {
        for pattern in ["%Y-%m-%d", "%d.%m.%Y"] {
            if let Ok(date) = NaiveDate::parse_from_str(&value[..10], pattern) {
                return Some(date_to_key(date));
            }
        }
    }

    let digits = DIGITS_RE
        .find_iter(value)
        .fold(String::with_capacity(8), |mut out, found| {
            out.push_str(found.as_str());
            out
        });
    (digits.len() == 8)
        .then_some(digits)
        .and_then(|digits| digits.parse::<i64>().ok())
}

#[inline]
fn date_to_key(date: NaiveDate) -> i64 {
    i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day())
}

pub(super) fn display_date(value: i64) -> String {
    let year = value / 10_000;
    let month = (value / 100) % 100;
    let day = value % 100;
    format!("{day:02}.{month:02}.{year:04}")
}

pub(super) fn normalize_well(value: &str) -> Option<i64> {
    let compact = value.replace(' ', "");
    let cleaned = WELL_SUFFIX_RE.replace_all(&compact, "");
    DIGITS_RE
        .find(cleaned.as_ref())
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use super::{display_date, normalize_well, parse_date_key};

    #[test]
    fn parse_date_key_supports_common_formats() {
        assert_eq!(parse_date_key("2025-03-14"), Some(20250314));
        assert_eq!(parse_date_key("14.03.2025 12:10:01"), Some(20250314));
        assert_eq!(parse_date_key("2025/03/14"), Some(20250314));
    }

    #[test]
    fn display_date_formats_yyyymmdd() {
        assert_eq!(display_date(20250314), "14.03.2025");
    }

    #[test]
    fn normalize_well_strips_suffix_and_spaces() {
        assert_eq!(normalize_well(" 123 -1 "), Some(123));
        assert_eq!(normalize_well("Скв. 456"), Some(456));
        assert_eq!(normalize_well("без номера"), None);
    }
}
