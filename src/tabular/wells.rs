//! Shared helpers for reading the common wells workbook.

use std::collections::BTreeSet;
use std::path::Path;

use ahash::AHashMap as HashMap;
use anyhow::{Context, Result};
use calamine::Data;

use crate::domain::Mest;
use crate::tabular::sheet::{AutoWorkbook, RowSchema, open_workbook, read_sheet_rows};

pub(crate) use crate::tabular::sheet::{HeaderMap, row_cell};

const MGPU_SHEET_NAMES: &[&str] = &[
    "МГПУ_ГП1",
    "МГПУ_ГП3",
    "МГПУ_ГП4",
    "МГПУ_ГП6",
    "МГПУ_ГП8",
    "МГПУ_ГП9",
];
const BNGKM_SHEET_NAMES: &[&str] = &["БНГКМ_ГП1", "БНГКМ_ГП2_1", "БНГКМ_ГП2_2", "БНГКМ_ГП3"];
const YUNGKM_SENOMAN_SHEET_NAMES: &[&str] = &["ЮНГКМ_сеноман"];
const YANGKM_SHEET_NAMES: &[&str] = &["ЯНГКМ"];
const HGKM_SHEET_NAMES: &[&str] = &["ХГКМ"];
const MGPU_NYDA_SHEET_NAMES: &[&str] = &["МГПУ_Ныда"];
const YUNGKM_APT_ALB_SHEET_NAMES: &[&str] = &["ЮНГКМ_апт-альб"];

pub(crate) fn sheet_names_for_mest(mest: Mest) -> &'static [&'static str] {
    match mest {
        Mest::Mgpu => MGPU_SHEET_NAMES,
        Mest::YungkmSenoman => YUNGKM_SENOMAN_SHEET_NAMES,
        Mest::Yangkm => YANGKM_SHEET_NAMES,
        Mest::Bngkm => BNGKM_SHEET_NAMES,
        Mest::Hgkm => HGKM_SHEET_NAMES,
        Mest::MgpuNyda => MGPU_NYDA_SHEET_NAMES,
        Mest::YungkmAptAlb => YUNGKM_APT_ALB_SHEET_NAMES,
    }
}

pub(crate) fn load_wells_for_mests<T, F>(
    path: &Path,
    mests: &[Mest],
    mut parse_row: F,
) -> Result<HashMap<Mest, HashMap<i64, T>>>
where
    F: FnMut(&HeaderMap, &[Data]) -> Option<(i64, T)>,
{
    let mut workbook = open_workbook(path)?;

    let mut out = HashMap::with_capacity(mests.len());
    let mut seen = BTreeSet::new();
    for &mest in mests {
        if !seen.insert(mest) {
            continue;
        }
        out.insert(
            mest,
            load_wells_for_sheets(&mut workbook, path, mest, &mut parse_row)?,
        );
    }

    Ok(out)
}

pub(crate) fn load_wells_for_mest<T, F>(
    path: &Path,
    mest: Mest,
    mut parse_row: F,
) -> Result<HashMap<i64, T>>
where
    F: FnMut(&HeaderMap, &[Data]) -> Option<(i64, T)>,
{
    let mut workbook = open_workbook(path)?;
    load_wells_for_sheets(&mut workbook, path, mest, &mut parse_row)
}

fn load_wells_for_sheets<T, F>(
    workbook: &mut AutoWorkbook,
    path: &Path,
    mest: Mest,
    parse_row: &mut F,
) -> Result<HashMap<i64, T>>
where
    F: FnMut(&HeaderMap, &[Data]) -> Option<(i64, T)>,
{
    let mut wells = HashMap::new();
    for &sheet_name in sheet_names_for_mest(mest) {
        let Some(rows) = read_sheet_rows(
            workbook,
            sheet_name,
            RowSchema::new(0, &["well"]),
            |headers, row| parse_row(headers, row),
        )
        .with_context(|| format!("Ошибка чтения листа {sheet_name} в {}", path.display()))?
        else {
            continue;
        };

        for (well, value) in rows {
            wells.insert(well, value);
        }
    }

    Ok(wells)
}
