//! Input loading helpers for the `SS` report.

use std::path::Path;

use ahash::AHashMap as HashMap;
use anyhow::Result;

use crate::domain::Mest;
use crate::tabular::wells::{load_wells_for_mests as load_well_maps, row_cell};
use crate::tabular::xlsx::{cell_to_f64, cell_to_i64, cell_to_shared_string};

use super::data::WellRow;

pub(super) fn load_wells_for_mests(
    path: &Path,
    mests: &[Mest],
) -> Result<HashMap<Mest, HashMap<i64, WellRow>>> {
    load_well_maps(path, mests, |headers, row| {
        let well = row_cell(headers, row, "well").and_then(cell_to_i64)?;
        Some((
            well,
            WellRow {
                gp: row_cell(headers, row, "gp").and_then(cell_to_shared_string),
                tkr: row_cell(headers, row, "tkr").and_then(cell_to_f64),
                pkr: row_cell(headers, row, "pkr").and_then(cell_to_f64),
            },
        ))
    })
}
