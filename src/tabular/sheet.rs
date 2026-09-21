//! Schema-based readers for workbook sheets.
//!
//! The project uses two recurring patterns when loading XLSX/XLSM files:
//! - row-oriented sheets with named columns (`well`, `qdop`, ...);
//! - table-like sheets where rows are read as display strings until a key
//!   column becomes empty.
//!
//! Keeping those mechanics here lets mode modules focus on business rules
//! instead of rebuilding header maps and row loops.

use std::io::{Read, Seek};
use std::path::Path;

use ahash::AHashMap as HashMap;
use anyhow::{Context, Result, anyhow, bail};
use calamine::{Data, Reader, Sheets, open_workbook_auto};

use crate::tabular::xlsx::{cell_to_display_string, cell_to_string};

pub(crate) type HeaderMap = HashMap<String, usize>;
pub(crate) type AutoWorkbook = Sheets<std::io::BufReader<std::fs::File>>;

#[derive(Debug, Clone, Copy)]
pub(crate) struct RowSchema<'a> {
    pub(crate) skip_rows: usize,
    pub(crate) required_headers: &'a [&'a str],
}

impl<'a> RowSchema<'a> {
    pub(crate) const fn new(skip_rows: usize, required_headers: &'a [&'a str]) -> Self {
        Self {
            skip_rows,
            required_headers,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TextSheetSchema<'a> {
    pub(crate) skip_rows: usize,
    pub(crate) stop_on_empty_header: &'a str,
}

impl<'a> TextSheetSchema<'a> {
    pub(crate) const fn new(skip_rows: usize, stop_on_empty_header: &'a str) -> Self {
        Self {
            skip_rows,
            stop_on_empty_header,
        }
    }
}

#[derive(Clone)]
pub(crate) struct TextSheet {
    pub(crate) headers: Vec<String>,
    pub(crate) headers_lc: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
}

pub(crate) fn open_workbook(path: &Path) -> Result<AutoWorkbook> {
    open_workbook_auto(path).with_context(|| format!("Не удалось открыть {}", path.display()))
}

pub(crate) fn read_sheet_rows<R, RS, T, F>(
    workbook: &mut R,
    sheet_name: &str,
    schema: RowSchema<'_>,
    mut parse_row: F,
) -> Result<Option<Vec<T>>>
where
    R: Reader<RS>,
    RS: Read + Seek,
    F: FnMut(&HeaderMap, &[Data]) -> Option<T>,
{
    let range = workbook
        .worksheet_range(sheet_name)
        .map_err(|err| anyhow!("Не найден лист {sheet_name}: {err:?}"))?;
    let mut rows = range.rows().skip(schema.skip_rows);
    let Some(header_row) = rows.next() else {
        return Ok(None);
    };

    let header_map = build_header_map(header_row);
    if !has_required_headers(&header_map, schema.required_headers) {
        return Ok(None);
    }

    Ok(Some(
        rows.filter_map(|row| parse_row(&header_map, row)).collect(),
    ))
}

pub(crate) fn load_text_sheet(
    path: &Path,
    sheet_name: &str,
    schema: TextSheetSchema<'_>,
) -> Result<TextSheet> {
    let mut workbook = open_workbook(path)?;
    read_text_sheet(&mut workbook, sheet_name, schema)
        .with_context(|| format!("Ошибка чтения листа {sheet_name} в {}", path.display()))
}

pub(crate) fn read_text_sheet<R, RS>(
    workbook: &mut R,
    sheet_name: &str,
    schema: TextSheetSchema<'_>,
) -> Result<TextSheet>
where
    R: Reader<RS>,
    RS: Read + Seek,
{
    let range = workbook
        .worksheet_range(sheet_name)
        .map_err(|err| anyhow!("Не найден лист {sheet_name}: {err:?}"))?;
    let mut rows = range.rows().skip(schema.skip_rows);
    let Some(header_row) = rows.next() else {
        bail!("Пустой лист {sheet_name}");
    };

    let mut headers = Vec::with_capacity(header_row.len());
    let mut headers_lc = Vec::with_capacity(header_row.len());
    for cell in header_row {
        let value = cell_to_display_string(cell);
        headers_lc.push(value.to_lowercase());
        headers.push(value);
    }
    let stop_header = schema.stop_on_empty_header.to_lowercase();
    let stop_idx = headers_lc
        .iter()
        .position(|value| value == &stop_header)
        .ok_or_else(|| anyhow!("Не найдена колонка {}", schema.stop_on_empty_header))?;

    let mut values = Vec::new();
    let mut data_started = false;
    for row in rows {
        let mut row_values = Vec::with_capacity(headers.len());
        let mut stop_is_empty = true;
        let mut has_values = false;

        for idx in 0..headers.len() {
            let value = row.get(idx).map(cell_to_display_string).unwrap_or_default();
            let trimmed = value.trim();
            if idx == stop_idx {
                stop_is_empty = trimmed.is_empty();
            }
            if !trimmed.is_empty() {
                has_values = true;
            }
            row_values.push(value);
        }

        if !data_started {
            if !has_values {
                continue;
            }
            if stop_is_empty {
                values.push(row_values);
                continue;
            }
            data_started = true;
            values.push(row_values);
            continue;
        }

        if stop_is_empty {
            break;
        }
        values.push(row_values);
    }

    Ok(TextSheet {
        headers,
        headers_lc,
        rows: values,
    })
}

pub(crate) fn row_cell<'a>(
    header_map: &HeaderMap,
    row: &'a [Data],
    name: &str,
) -> Option<&'a Data> {
    let idx = header_map.get(name)?;
    row.get(*idx)
}

fn build_header_map(header_row: &[Data]) -> HeaderMap {
    header_row
        .iter()
        .enumerate()
        .filter_map(|(idx, cell)| cell_to_string(cell).map(|name| (name.to_lowercase(), idx)))
        .collect()
}

fn has_required_headers(header_map: &HeaderMap, required_headers: &[&str]) -> bool {
    required_headers
        .iter()
        .all(|name| header_map.contains_key(*name))
}
