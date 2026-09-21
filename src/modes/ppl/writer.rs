//! XLSX rendering helpers for the `PPL` report.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ahash::AHashMap as HashMap;
use anyhow::{Context, Result};
use rust_xlsxwriter::{Color, Format, Workbook, Worksheet, XlsxError};

use crate::domain::Mest;
use crate::paths;

use super::data::{SourceRow, WellRow};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PivotKey {
    gp: Option<Arc<str>>,
    plast: Option<Arc<str>>,
    well: i64,
}

#[derive(Debug, Clone, Default)]
struct AggCell {
    sum: f64,
    count: usize,
}

/// Агрегаты одной ячейки сводной таблицы (скважина × месяц): значения
/// обоих листов и признак замера для подсветки.
#[derive(Debug, Clone, Default)]
struct PivotCell {
    ppl: AggCell,
    pst: AggCell,
    zamer: AggCell,
}

type PivotCells = HashMap<PivotKey, BTreeMap<i64, PivotCell>>;
/// Селектор агрегата для листа (ppl или pst).
type SheetSelect = fn(&PivotCell) -> &AggCell;

struct PivotSheetSpec<'a> {
    cells: &'a PivotCells,
    select: SheetSelect,
    use_gp: bool,
    header: &'a Format,
    highlight: &'a Format,
}

impl AggCell {
    fn add(&mut self, value: Option<f64>) {
        if let Some(value) = value {
            self.sum += value;
            self.count += 1;
        }
    }

    fn average(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count as f64)
        }
    }
}

pub(super) fn build_output_path(mest: Mest, debug_mode: bool) -> PathBuf {
    let stem = format!("Статика_{}.xlsx", mest.output_label());
    paths::report_output_dir("Статика", "Статика", debug_mode).join(stem)
}

pub(super) fn write_workbook(
    output_path: &Path,
    wells: &HashMap<i64, WellRow>,
    rows: Vec<SourceRow>,
    use_gp: bool,
) -> Result<()> {
    paths::ensure_parent_dir(output_path)?;

    // key_set хранит порядок первого появления ключа до финальной
    // сортировки; сама карта ячеек служит и множеством «уже видели».
    let mut key_set = Vec::<PivotKey>::new();
    let mut date_cols = BTreeSet::<i64>::new();
    let mut cells: PivotCells = HashMap::new();

    for row in rows {
        let well_meta = wells.get(&row.well);
        let plast = well_meta.and_then(|meta| meta.plast.clone());
        if plast.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        let key = PivotKey {
            gp: well_meta.and_then(|meta| meta.gp.clone()),
            plast,
            well: row.well,
        };
        if !cells.contains_key(&key) {
            key_set.push(key.clone());
        }

        date_cols.insert(row.date_yyyymm);
        let cell = cells
            .entry(key)
            .or_default()
            .entry(row.date_yyyymm)
            .or_default();
        cell.ppl.add(row.ppl);
        cell.pst.add(row.pst);
        cell.zamer.add(row.zamer);
    }

    key_set.sort_by(|left, right| {
        if use_gp {
            left.gp
                .cmp(&right.gp)
                .then(left.well.cmp(&right.well))
                .then(left.plast.cmp(&right.plast))
        } else {
            left.well
                .cmp(&right.well)
                .then(left.plast.cmp(&right.plast))
        }
    });

    let highlight = Format::new().set_background_color(Color::RGB(0xFFFACD));
    let header = Format::new().set_bold();

    let mut workbook = Workbook::new();
    let sheets: [(&str, SheetSelect); 2] = [("ppl", |cell| &cell.ppl), ("pst", |cell| &cell.pst)];
    for (name, select) in sheets {
        let sheet = workbook.add_worksheet().set_name(name)?;
        write_pivot_sheet(
            sheet,
            &key_set,
            &date_cols,
            PivotSheetSpec {
                cells: &cells,
                select,
                use_gp,
                header: &header,
                highlight: &highlight,
            },
        )?;
    }

    workbook
        .save(output_path)
        .with_context(|| format!("Не удалось сохранить {}", output_path.display()))?;
    Ok(())
}

fn write_pivot_sheet(
    sheet: &mut Worksheet,
    keys: &[PivotKey],
    date_cols: &BTreeSet<i64>,
    spec: PivotSheetSpec<'_>,
) -> Result<(), XlsxError> {
    let key_headers: &[&str] = if spec.use_gp {
        &["gp", "plast", "well"]
    } else {
        &["plast", "well"]
    };

    for (col, name) in key_headers.iter().enumerate() {
        sheet.write_with_format(0, col as u16, *name, spec.header)?;
    }
    for (offset, date) in date_cols.iter().enumerate() {
        let date_label = date.to_string();
        sheet.write_with_format(
            0,
            (key_headers.len() + offset) as u16,
            date_label.as_str(),
            spec.header,
        )?;
    }

    for (row_idx, key) in keys.iter().enumerate() {
        let row = (row_idx + 1) as u32;
        let mut col = 0u16;

        if spec.use_gp {
            sheet.write_string(row, col, key.gp.as_deref().unwrap_or(""))?;
            col += 1;
        }

        sheet.write_string(row, col, key.plast.as_deref().unwrap_or(""))?;
        col += 1;
        sheet.write_number(row, col, key.well as f64)?;

        // ячейки ключа берутся один раз, а не хэшируются на каждую дату
        let key_cells = spec.cells.get(key);
        for (offset, date) in date_cols.iter().enumerate() {
            let col_idx = (key_headers.len() + offset) as u16;
            let Some(cell) = key_cells.and_then(|dates| dates.get(date)) else {
                continue;
            };
            if let Some(value) = (spec.select)(cell).average() {
                let highlight_cell = cell
                    .zamer
                    .average()
                    .is_some_and(|avg| (avg - 1.0).abs() < f64::EPSILON);

                if highlight_cell {
                    sheet.write_with_format(row, col_idx, value, spec.highlight)?;
                } else {
                    sheet.write_number(row, col_idx, value)?;
                }
            }
        }
    }

    Ok(())
}
