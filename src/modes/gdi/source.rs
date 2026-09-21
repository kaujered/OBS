//! Input loading helpers for the DBF-based `GDI` report.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ahash::AHashMap as HashMap;
use anyhow::Result;

use crate::domain::Mest;
use crate::paths;
use crate::tabular::wells::{load_wells_for_mests as load_well_maps, row_cell};
use crate::tabular::xlsx::{cell_to_f64, cell_to_i64, cell_to_shared_string};

use super::data::{KSMEST_GAUGE_FIELDS, KsmestLast, PlastLast, Stand1Last, Stand2Row, WellRow};

use crate::tabular::dbf::DbfTable;

pub(super) fn load_wells_for_mests(
    path: &Path,
    mests: &[Mest],
) -> Result<HashMap<Mest, HashMap<i64, WellRow>>> {
    Ok(load_well_maps(path, mests, |headers, row| {
        let well = row_cell(headers, row, "well").and_then(cell_to_i64)?;
        Some((
            well,
            WellRow {
                plast: row_cell(headers, row, "plast").and_then(cell_to_shared_string),
                gp: row_cell(headers, row, "gp").and_then(cell_to_shared_string),
                depr: row_cell(headers, row, "depr").and_then(cell_to_f64),
            },
        ))
    })?
    .into_iter()
    .map(|(mest, wells)| (mest, wells.into_iter().collect()))
    .collect())
}

pub(super) fn resolve_wells_path(debug_mode: bool) -> Result<PathBuf> {
    let candidates = paths::skvgdn_candidates(debug_mode)?;
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow::anyhow!("Не найден СкважиныГДН.xlsx для KOTS-ветки GDI"))
}

pub(super) fn load_stand2_rows(
    path: &Path,
    selected_codes: &BTreeSet<i32>,
    date_filter: i64,
    include_rejected: bool,
) -> Result<HashMap<i32, Vec<Stand2Row>>> {
    let mut table = DbfTable::open(path)?;
    let st1 = table.field("ST1")?;
    let st3 = table.field("ST3")?;
    let bw1 = table.field("BW1")?;
    let brk = table.field("BRK")?;
    let bz1 = table.field("BZ1")?;
    let bz3 = table.field("BZ3")?;
    let bz4 = table.field("BZ4")?;
    let bz7 = table.field("BZ7")?;
    let bz10 = table.field("BZ10")?;
    let bz14 = table.field("BZ14")?;
    let bz15 = table.field("BZ15")?;
    let bw2 = table.field("BW2")?;
    let bw4 = table.field("BW4")?;

    let parsed = table.par_filter_map(|record| {
        let mest_code = record.i32(st1)?;
        if !selected_codes.contains(&mest_code) {
            return None;
        }
        let (well, date_key) = (record.i64(st3)?, record.i64(bw1)?);
        if date_key < date_filter {
            return None;
        }
        if !include_rejected && !matches!(record.f64(brk), None | Some(0.0)) {
            return None;
        }
        Some((
            mest_code,
            Stand2Row {
                regime_no: record.f64(bz1),
                washer: record.f64(bz3),
                well,
                date_key,
                thp: record.f64(bz4),
                flo: record
                    .f64(bz10)
                    .map(|value| ((value * 10.0).round() / 10.0) * 1000.0),
                bhp: record.f64(bz7),
                pst: record.f64(bw2),
                ppl: record.f64(bw4),
                water: record.f64(bz14),
                sand: record.f64(bz15),
            },
        ))
    })?;

    let mut rows_by_mest = HashMap::<i32, Vec<Stand2Row>>::new();
    for (mest_code, row) in parsed {
        rows_by_mest.entry(mest_code).or_default().push(row);
    }
    Ok(rows_by_mest)
}

pub(super) fn load_ksmest_last(
    path: &Path,
    selected_codes: &BTreeSet<i32>,
) -> Result<HashMap<i32, HashMap<i64, KsmestLast>>> {
    let mut table = DbfTable::open(path)?;
    let ba1 = table.field("BA1")?;
    let be1 = table.field("BE1")?;
    let mut gauges = Vec::with_capacity(KSMEST_GAUGE_FIELDS.len());
    for name in KSMEST_GAUGE_FIELDS {
        gauges.push(table.field(name)?);
    }

    let parsed = table.par_filter_map(|record| {
        let mest_code = record.i32(ba1)?;
        if !selected_codes.contains(&mest_code) {
            return None;
        }
        let well = record.i64(be1)?;
        let mut gauge_values = [None; KSMEST_GAUGE_FIELDS.len()];
        for (value, &field) in gauge_values.iter_mut().zip(&gauges) {
            *value = record.f64(field);
        }
        Some((mest_code, well, KsmestLast { gauge_values }))
    })?;

    // порядок файла сохранён: последняя запись по скважине побеждает
    let mut by_mest = HashMap::<i32, HashMap<i64, KsmestLast>>::new();
    for (mest_code, well, row) in parsed {
        by_mest.entry(mest_code).or_default().insert(well, row);
    }
    Ok(by_mest)
}

pub(super) fn load_plast_last(
    path: &Path,
    selected_codes: &BTreeSet<i32>,
) -> Result<HashMap<i32, HashMap<i64, PlastLast>>> {
    let mut table = DbfTable::open(path)?;
    let bbl1 = table.field("BBL1")?;
    let bbl3 = table.field("BBL3")?;
    let bl4t = table.field("BL4T")?;

    let parsed = table.par_filter_map(|record| {
        let mest_code = record.i32(bbl1)?;
        if !selected_codes.contains(&mest_code) {
            return None;
        }
        let well = record.i64(bbl3)?;
        Some((
            mest_code,
            well,
            PlastLast {
                tpl_kelvin: record.f64(bl4t),
            },
        ))
    })?;

    let mut by_mest = HashMap::<i32, HashMap<i64, PlastLast>>::new();
    for (mest_code, well, row) in parsed {
        by_mest.entry(mest_code).or_default().insert(well, row);
    }
    Ok(by_mest)
}

pub(super) fn load_stand1_last(
    path: &Path,
    selected_codes: &BTreeSet<i32>,
) -> Result<HashMap<i32, HashMap<i64, Stand1Last>>> {
    let mut table = DbfTable::open(path)?;
    let bby1 = table.field("BBY1")?;
    let by1 = table.field("BY1")?;
    let bby3 = table.field("BBY3")?;
    let by2 = table.field("BY2")?;
    let by3 = table.field("BY3")?;

    let parsed = table.par_filter_map(|record| {
        let mest_code = record.i32(bby1)?;
        if !selected_codes.contains(&mest_code) || record.i32(by1) != Some(9) {
            return None;
        }
        let well = record.i64(bby3)?;
        Some((
            mest_code,
            well,
            Stand1Last {
                c: record.f64(by2),
                n: record.f64(by3),
            },
        ))
    })?;

    let mut by_mest = HashMap::<i32, HashMap<i64, Stand1Last>>::new();
    for (mest_code, well, row) in parsed {
        by_mest.entry(mest_code).or_default().insert(well, row);
    }
    Ok(by_mest)
}
