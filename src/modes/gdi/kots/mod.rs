//! Excel/KOTS compatibility branch for the `GDI` report.
//!
//! Layout:
//! - `data` stores row/table models;
//! - `data` also converts source tables into report rows;
//! - `writer` builds the final workbook;
//! - `source` loads Excel sheets and resolves candidate files.

mod cache;
mod data;
mod source;
mod writer;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use calamine::Data;
use chrono::{Datelike, Local};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use regex::Regex;

use self::cache::load_rows_cached;
use crate::modes::gdi::limits::{apply_limit_results, retain_last_date_per_well};

use crate::domain::Mest;
use crate::paths;

use self::data::*;
use self::source::*;
use self::writer::*;

static DIGITS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d+").expect("regex"));
static WELL_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"-\d+").expect("regex"));

pub fn execute_bngkm(
    year: i32,
    month: u32,
    day: u32,
    wells_path: &Path,
    debug_mode: bool,
    export_charts: bool,
    filter_date: Option<i64>,
) -> Result<PathBuf> {
    let wells = load_wells(wells_path, Mest::Bngkm)?;
    let current_year = Local::now().year();
    if !debug_mode {
        sync_bngkm_kots_sources(current_year)?;
    }
    let cache_dir = kots_cache_dir();

    // BNGKM data is spread across yearly workbooks plus special PK/GP-3 files.
    // Файлы независимы: разбираются параллельно, порядок лет в результате
    // сохраняется; разобранные строки кэшируются на диске в папке шаблонов
    // (книги прошлых лет не меняются, их повторный разбор — потеря времени).
    let (year_rows, (pk_rows, gp3_rows)) = rayon::join(
        || -> Result<Vec<Vec<OutputRow>>> {
            (2019..=current_year)
                .into_par_iter()
                .map(|src_year| {
                    let Some(path) =
                        first_existing_file(&bngkm_year_candidates(src_year, debug_mode))
                    else {
                        return Ok(Vec::new());
                    };
                    load_rows_cached(
                        &cache_dir,
                        &format!("БНГКМ_{src_year}"),
                        &path,
                        wells_path,
                        || {
                            let table = load_sheet_table(&path, "svod", 0, "№ скв")?;
                            parse_bngkm_table(&table, src_year, false, false, &wells)
                        },
                    )
                })
                .collect()
        },
        || {
            rayon::join(
                || -> Result<Vec<OutputRow>> {
                    let Some(path) = first_existing_file(&bngkm_pk_candidates(debug_mode)) else {
                        return Ok(Vec::new());
                    };
                    load_rows_cached(&cache_dir, "БНГКМ_ПК", &path, wells_path, || {
                        let table = load_sheet_table(&path, "svod", 0, "№ скв")?;
                        parse_bngkm_table(&table, 9999, true, false, &wells)
                    })
                },
                || -> Result<Vec<OutputRow>> {
                    let Some(path) = first_existing_file(&bngkm_gp3_candidates(debug_mode)) else {
                        return Ok(Vec::new());
                    };
                    load_rows_cached(&cache_dir, "БНГКМ_ГП3", &path, wells_path, || {
                        let table = load_sheet_table(&path, "BAZA_TP_GP-3", 1, "Скв")?;
                        parse_bngkm_table(&table, 9999, false, true, &wells)
                    })
                },
            )
        },
    );
    let mut rows: Vec<OutputRow> = year_rows?.into_iter().flatten().collect();
    rows.extend(pk_rows?);
    rows.extend(
        gp3_rows?
            .into_iter()
            .filter(|row| row.flo.unwrap_or(0.0) != 0.0),
    );
    if rows.is_empty() {
        bail!("Не найдены источники KOTS для БНГКМ.");
    }
    retain_last_date_per_well(&mut rows, |row| (row.well, row.date_key));
    let date_filter = i64::from(year) * 10_000 + i64::from(month) * 100 + i64::from(day);
    rows.retain(|row| row.date_key >= date_filter);
    rows.sort_by(|a, b| {
        a.well.cmp(&b.well).then(compare_regime(
            a.regime_no.as_deref(),
            b.regime_no.as_deref(),
        ))
    });
    dedup_rows(&mut rows);
    let path = paths::report_output_dir("ГДИ", "ГДИ", debug_mode).join("mest4_КОЦ.xlsx");
    apply_limit_results(&mut rows);
    write_workbook(&path, &rows, export_charts, filter_date)?;
    Ok(path)
}

pub fn execute_hgkm(
    year: i32,
    month: u32,
    day: u32,
    wells_path: &Path,
    debug_mode: bool,
    export_charts: bool,
    filter_date: Option<i64>,
) -> Result<PathBuf> {
    let wells = load_wells(wells_path, Mest::Hgkm)?;
    let source = required_existing(&hgkm_candidates(debug_mode), "Освоение ХГКМ.xlsm")?;
    let mut rows = load_rows_cached(&kots_cache_dir(), "ХГКМ", &source, wells_path, || {
        let table = load_sheet_table(&source, "svod", 0, "№ скв")?;
        parse_hgkm_table(&table, &wells)
    })?;
    if rows.is_empty() {
        bail!("Не найдены данные KOTS для ХГКМ.");
    }
    retain_last_date_per_well(&mut rows, |row| (row.well, row.date_key));
    let date_filter = i64::from(year) * 10_000 + i64::from(month) * 100 + i64::from(day);
    rows.retain(|row| row.date_key >= date_filter);
    rows.sort_by(|a, b| {
        a.well.cmp(&b.well).then(compare_regime(
            a.regime_no.as_deref(),
            b.regime_no.as_deref(),
        ))
    });
    let path = paths::report_output_dir("ГДИ", "ГДИ", debug_mode).join("mest5_КОЦ.xlsx");
    apply_limit_results(&mut rows);
    write_workbook(&path, &rows, export_charts, filter_date)?;
    Ok(path)
}
