//! Native implementation of the `SS` report.
//!
//! The mode combines measurements from `eer1.dbf` with well metadata from
//! `СкважиныГДН.xlsx` and writes one workbook per selected field.

mod data;
mod source;
mod writer;

pub use data::Request;

use std::collections::BTreeSet;
use std::path::PathBuf;

use ahash::AHashMap as HashMap;
use anyhow::{Context, Result, bail};
use rayon::prelude::*;

use crate::domain::Mest;
use crate::paths;

use self::data::{OutputRow, WellRow, get_vyst, get_z};
use self::source::load_wells_for_mests;
use self::writer::{build_output_path, write_workbook};
use crate::tabular::cache::{SourceCache, file_stamp};
use crate::tabular::dbf::{DbfField, DbfRecordView, DbfTable};

type SsCacheKey = (
    Option<(std::time::SystemTime, u64)>,
    Option<(std::time::SystemTime, u64)>,
    BTreeSet<i32>,
    i64,
    bool,
);

static ROWS_CACHE: SourceCache<SsCacheKey, HashMap<i32, Vec<OutputRow>>> = SourceCache::new();

pub fn execute(request: &Request) -> Result<Vec<PathBuf>> {
    if request.mests.is_empty() {
        bail!("Выберите хотя бы одно месторождение.");
    }

    let paths = paths::resolve_ss_paths(request.test_mode, request.debug_mode)?;
    // МГПУ absorbs Ныда: load the source fields of every selected field so the merged
    // sheet has both, while a separately selected Ныда still gets its own workbook.
    let load_mests = request
        .mests
        .iter()
        .flat_map(|mest| mest.merge_sources().iter().copied())
        .collect::<BTreeSet<_>>();
    let selected_codes = load_mests
        .iter()
        .map(|mest| mest.code())
        .collect::<BTreeSet<_>>();
    let date_filter = i64::from(request.year) * 100 + i64::from(request.month);

    let cache_key = (
        file_stamp(&paths.eer1_dbf),
        file_stamp(&paths.wells_xlsx),
        selected_codes.clone(),
        date_filter,
        request.only_last,
    );
    let rows_by_mest = ROWS_CACHE.get_or_load(cache_key, || {
        load_rows_by_mest(
            &paths,
            &load_mests,
            &selected_codes,
            date_filter,
            request.only_last,
        )
    })?;

    let created = request
        .mests
        .par_iter()
        .map(|mest| {
            // Merge the field with its sources (МГПУ + Ныда) onto a single sheet.
            let mut rows = mest
                .merge_sources()
                .iter()
                .filter_map(|src| rows_by_mest.get(&src.code()))
                .flat_map(|rows| rows.iter().cloned())
                .collect::<Vec<_>>();
            rows.sort_by(|left, right| {
                left.date
                    .cmp(&right.date)
                    .then(left.gp.cmp(&right.gp))
                    .then(left.well.cmp(&right.well))
            });
            process_mest(*mest, request, &rows)
        })
        .collect::<Result<Vec<_>>>()?;

    if request.debug_mode {
        return Ok(created);
    }

    paths::replicate_output_files(
        &created,
        &paths::gis_output_dir("Суточные сводки"),
        paths::OutputGroup::DailySummary,
    )
}

/// Строки сводки по месторождениям: eer1.dbf читается с хвоста (записи
/// дописываются хронологически, нужны только даты не раньше отчётной),
/// разбор параллельный и только по нужным полям.
fn load_rows_by_mest(
    paths: &paths::SsPaths,
    load_mests: &BTreeSet<Mest>,
    selected_codes: &BTreeSet<i32>,
    date_filter: i64,
    only_last: bool,
) -> Result<HashMap<i32, Vec<OutputRow>>> {
    let wells_by_mest = load_wells_for_mests(
        &paths.wells_xlsx,
        &load_mests.iter().copied().collect::<Vec<_>>(),
    )?;
    let wells_by_code = load_mests
        .iter()
        .map(|mest| {
            let wells = wells_by_mest.get(mest).ok_or_else(|| {
                anyhow::anyhow!("Не найдены данные СкважиныГДН.xlsx для {}", mest.ui_label())
            })?;
            Ok((mest.code(), wells))
        })
        .collect::<Result<HashMap<i32, &HashMap<i64, WellRow>>>>()?;

    let mut table = DbfTable::open(&paths.eer1_dbf)
        .with_context(|| format!("Не удалось прочитать {}", paths.eer1_dbf.display()))?;
    let fields = SsFields::resolve(&table)
        .with_context(|| format!("Не удалось прочитать {}", paths.eer1_dbf.display()))?;

    let parsed = table
        .par_filter_map_from_end(
            |record| {
                record
                    .i64(fields.bs1)
                    .is_some_and(|date| date >= date_filter)
            },
            |record| {
                let mest_code = record.i32(fields.bb1)?;
                if !selected_codes.contains(&mest_code) {
                    return None;
                }
                let (well, is_prod, date) = (
                    record.i64(fields.bb3)?,
                    record.i32(fields.bb4)?,
                    record.i64(fields.bs1)?,
                );
                if is_prod != 1 || date < date_filter || (only_last && date != date_filter) {
                    return None;
                }

                let meta = wells_by_code
                    .get(&mest_code)
                    .and_then(|wells| wells.get(&well));
                Some((
                    mest_code,
                    build_output_row(record, &fields, meta, well, date),
                ))
            },
        )
        .with_context(|| format!("Не удалось прочитать {}", paths.eer1_dbf.display()))?;

    let mut rows_by_mest = HashMap::<i32, Vec<OutputRow>>::new();
    for (mest_code, row) in parsed {
        rows_by_mest.entry(mest_code).or_default().push(row);
    }
    Ok(rows_by_mest)
}

/// Строка сводки из записи DBF; скорость потока считается по z-фактору,
/// когда у скважины заданы критические параметры (Ткр, Ркр).
fn build_output_row(
    record: &DbfRecordView<'_>,
    fields: &SsFields,
    meta: Option<&WellRow>,
    well: i64,
    date: i64,
) -> OutputRow {
    let rbuf_ata = record.f64(fields.bs15);
    let tus_c = record.f64(fields.bs16);
    let q_tmsut = record.f64(fields.bs21);
    let z = match (
        rbuf_ata,
        tus_c,
        meta.and_then(|v| v.tkr),
        meta.and_then(|v| v.pkr),
    ) {
        (Some(p), Some(t), Some(tkr), Some(pkr)) if tkr != 0.0 && pkr != 0.0 => {
            Some(get_z(p, t, tkr, pkr))
        }
        _ => None,
    };
    let speed = match (rbuf_ata, tus_c, z, q_tmsut) {
        (Some(p), Some(t), Some(z), Some(q)) if p != 0.0 => {
            get_vyst(p, t, z, q, 0.1).unwrap_or(0.0)
        }
        _ => 0.0,
    };

    OutputRow {
        date,
        gp: meta.and_then(|v| v.gp.clone()),
        well,
        rbuf_ata,
        rzat_ata: record.f64(fields.bs11),
        depr_ata: record.f64(fields.bs22),
        rmk_ata: record.f64(fields.bs19),
        rshl_ata: record.f64(fields.bs12),
        tus_c,
        rvh_ata: record.f64(fields.bs13),
        tvh_c: record.f64(fields.bs17),
        q_tmsut,
        speed,
        q_water: record.f64(fields.bs23),
        q_sand: record.f64(fields.bs24),
        udk_mm: record.f64(fields.bs27),
        washer_mm: record.f64(fields.bs26),
        smzd: record.f64(fields.bs18),
    }
}

/// Поля суточной сводки в eer1.dbf.
struct SsFields {
    bb1: DbfField,
    bb3: DbfField,
    bb4: DbfField,
    bs1: DbfField,
    bs11: DbfField,
    bs12: DbfField,
    bs13: DbfField,
    bs15: DbfField,
    bs16: DbfField,
    bs17: DbfField,
    bs18: DbfField,
    bs19: DbfField,
    bs21: DbfField,
    bs22: DbfField,
    bs23: DbfField,
    bs24: DbfField,
    bs26: DbfField,
    bs27: DbfField,
}

impl SsFields {
    fn resolve(table: &DbfTable) -> Result<Self> {
        Ok(Self {
            bb1: table.field("BB1")?,
            bb3: table.field("BB3")?,
            bb4: table.field("BB4")?,
            bs1: table.field("BS1")?,
            bs11: table.field("BS11")?,
            bs12: table.field("BS12")?,
            bs13: table.field("BS13")?,
            bs15: table.field("BS15")?,
            bs16: table.field("BS16")?,
            bs17: table.field("BS17")?,
            bs18: table.field("BS18")?,
            bs19: table.field("BS19")?,
            bs21: table.field("BS21")?,
            bs22: table.field("BS22")?,
            bs23: table.field("BS23")?,
            bs24: table.field("BS24")?,
            bs26: table.field("BS26")?,
            bs27: table.field("BS27")?,
        })
    }
}

fn process_mest(mest: Mest, request: &Request, rows: &[OutputRow]) -> Result<PathBuf> {
    let output_path = build_output_path(
        mest,
        request.year,
        request.month,
        request.only_last,
        request.debug_mode,
    );
    write_workbook(&output_path, rows)?;
    Ok(output_path)
}
