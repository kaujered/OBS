//! Native implementation of the `PPL` report.
//!
//! High-level flow:
//! 1. resolve `plast.dbf` and `СкважиныГДН.xlsx`;
//! 2. preload well metadata for selected fields;
//! 3. stream DBF rows once and group parsed rows by field code;
//! 4. aggregate values into pivot-like sheets;
//! 5. write one workbook per field.

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

use self::data::{SourceRow, WellRow};
use self::source::load_wells_for_mests;
use self::writer::{build_output_path, write_workbook};
use crate::tabular::cache::{SourceCache, file_stamp};
use crate::tabular::dbf::DbfTable;

type PplCacheKey = (
    Option<(std::time::SystemTime, u64)>,
    BTreeSet<i32>,
    i64,
    bool,
);

static ROWS_CACHE: SourceCache<PplCacheKey, HashMap<i32, Vec<SourceRow>>> = SourceCache::new();

pub fn execute(request: &Request) -> Result<Vec<PathBuf>> {
    if request.mests.is_empty() {
        bail!("Выберите хотя бы одно месторождение.");
    }

    let paths = paths::resolve_ppl_paths(request.test_mode, request.debug_mode)?;
    // МГПУ absorbs Ныда: load the source fields of every selected field so the merged
    // workbook has both, while a separately selected Ныда still gets its own workbook.
    let load_mests = request
        .mests
        .iter()
        .flat_map(|mest| mest.merge_sources().iter().copied())
        .collect::<BTreeSet<_>>();
    let wells_by_mest = load_wells_for_mests(
        &paths.wells_xlsx,
        &load_mests.iter().copied().collect::<Vec<_>>(),
    )?;
    let selected_codes = load_mests
        .iter()
        .map(|mest| mest.code())
        .collect::<BTreeSet<_>>();
    let start_date = i64::from(request.year) * 10_000 + 101;
    let year_filter = i64::from(request.year);

    let cache_key = (
        file_stamp(&paths.plast_dbf),
        selected_codes.clone(),
        start_date,
        request.only_last,
    );
    let rows_by_mest = ROWS_CACHE.get_or_load(cache_key, || {
        load_rows_by_mest(
            &paths.plast_dbf,
            &selected_codes,
            start_date,
            request.only_last,
            year_filter,
        )
    })?;

    let jobs = request
        .mests
        .iter()
        .map(|mest| {
            // Merge the field with its sources (МГПУ + Ныда) into one workbook.
            let mut wells = HashMap::<i64, WellRow>::new();
            let mut rows = Vec::<SourceRow>::new();
            for src in mest.merge_sources() {
                let src_wells = wells_by_mest.get(src).ok_or_else(|| {
                    anyhow::anyhow!("Не найдены данные СкважиныГДН.xlsx для {}", src.ui_label())
                })?;
                wells.extend(src_wells.iter().map(|(well, row)| (*well, row.clone())));
                if let Some(src_rows) = rows_by_mest.get(&src.code()) {
                    rows.extend(src_rows.iter().cloned());
                }
            }
            Ok((*mest, rows, wells))
        })
        .collect::<Result<Vec<_>>>()?;

    let created = jobs
        .into_par_iter()
        .map(|(mest, rows, wells)| process_mest(mest, request, rows, &wells))
        .collect::<Result<Vec<_>>>()?;

    if request.debug_mode {
        return Ok(created);
    }

    paths::replicate_output_files(
        &created,
        &paths::gis_output_dir("Статика"),
        paths::OutputGroup::Statika,
    )
}

/// Строки статики по месторождениям: plast.dbf не упорядочен по датам,
/// поэтому читается целиком, но параллельно и только по нужным полям.
fn load_rows_by_mest(
    plast_dbf: &std::path::Path,
    selected_codes: &BTreeSet<i32>,
    start_date: i64,
    only_last: bool,
    year_filter: i64,
) -> Result<HashMap<i32, Vec<SourceRow>>> {
    let mut table = DbfTable::open(plast_dbf)
        .with_context(|| format!("Не удалось прочитать {}", plast_dbf.display()))?;
    let bbl1 = table.field("BBL1")?;
    let bbl3 = table.field("BBL3")?;
    let bbl4 = table.field("BBL4")?;
    let bl1 = table.field("BL1")?;
    let bl3 = table.field("BL3")?;
    let bl4 = table.field("BL4")?;
    let bl5 = table.field("BL5")?;

    let parsed = table
        .par_filter_map(|record| {
            let mest_code = record.i32(bbl1)?;
            if !selected_codes.contains(&mest_code) {
                return None;
            }
            let (well, is_prod, date_raw) =
                (record.i64(bbl3)?, record.i32(bbl4)?, record.i64(bl1)?);
            if is_prod != 1 || date_raw < start_date {
                return None;
            }
            let date_yyyymm = date_raw / 100;
            if only_last && date_yyyymm / 100 != year_filter {
                return None;
            }
            Some((
                mest_code,
                SourceRow {
                    well,
                    date_yyyymm,
                    pst: record.f64(bl3),
                    ppl: record.f64(bl4),
                    zamer: record.f64(bl5),
                },
            ))
        })
        .with_context(|| format!("Не удалось прочитать {}", plast_dbf.display()))?;

    let mut rows_by_mest = HashMap::<i32, Vec<SourceRow>>::new();
    for (mest_code, row) in parsed {
        rows_by_mest.entry(mest_code).or_default().push(row);
    }
    Ok(rows_by_mest)
}

fn process_mest(
    mest: Mest,
    request: &Request,
    rows: Vec<SourceRow>,
    wells: &HashMap<i64, WellRow>,
) -> Result<PathBuf> {
    let use_gp = wells
        .values()
        .any(|row| row.gp.as_deref().is_some_and(|value| !value.is_empty()));
    let output_path = build_output_path(mest, request.debug_mode);

    write_workbook(&output_path, wells, rows, use_gp)?;
    Ok(output_path)
}
