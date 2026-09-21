//! XLSX rendering helpers for the `SS` report.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rust_xlsxwriter::Workbook;

use crate::domain::Mest;
use crate::paths;
use crate::tabular::report_xlsx::{
    configure_ss_sheet, ss_formats, ss_row_format, write_headers, write_number,
    write_optional_number, write_optional_text,
};

use super::data::OutputRow;

pub(super) fn build_output_path(
    mest: Mest,
    year: i32,
    month: u32,
    only_last: bool,
    debug_mode: bool,
) -> PathBuf {
    let stem = if only_last {
        format!("Сводка_{}_за_{year}{month:02}.xlsx", mest.output_label())
    } else {
        format!("Сводка_{}_с_{year}{month:02}.xlsx", mest.output_label())
    };
    paths::report_output_dir("Суточные сводки", "Сводка", debug_mode).join(stem)
}

pub(super) fn write_workbook(output_path: &Path, rows: &[OutputRow]) -> Result<()> {
    paths::ensure_parent_dir(output_path)?;

    let mut workbook = Workbook::new();
    let headers = [
        "Дата",
        "ГП",
        "№ скв",
        "Рбуф, ата",
        "Рзат, ата",
        "депр, ата",
        "Рмк, ата",
        "Ршл, ата",
        "Тус, С",
        "Рвх, ата",
        "Твх, С",
        "Q, тм3/сут",
        "Скорость",
        "вода, м3",
        "мех.пр, м3",
        "УДК, мм",
        "шайба, мм",
        "Коэф. смзд",
    ];
    let formats = ss_formats();
    let worksheet = configure_ss_sheet(&mut workbook, &headers)?;

    write_headers(worksheet, &headers, &formats.header, None)?;

    let mut prev_date = None;
    for (idx, row) in rows.iter().enumerate() {
        let excel_row = (idx + 1) as u32;
        let top_border = prev_date.is_some_and(|prev| prev != row.date);
        let grey_row = row.smzd.is_some_and(|value| value == 0.0);
        let format = ss_row_format(&formats, grey_row, top_border);

        write_number(worksheet, excel_row, 0, row.date as f64, format)?;
        write_optional_text(worksheet, excel_row, 1, row.gp.as_deref(), format)?;
        write_number(worksheet, excel_row, 2, row.well as f64, format)?;
        // числовые колонки в порядке заголовков (Рбуф..Коэф. смзд)
        let numbers = [
            row.rbuf_ata,
            row.rzat_ata,
            row.depr_ata,
            row.rmk_ata,
            row.rshl_ata,
            row.tus_c,
            row.rvh_ata,
            row.tvh_c,
            row.q_tmsut,
            Some(row.speed),
            row.q_water,
            row.q_sand,
            row.udk_mm,
            row.washer_mm,
            row.smzd,
        ];
        for (offset, value) in numbers.into_iter().enumerate() {
            write_optional_number(worksheet, excel_row, (3 + offset) as u16, value, format)?;
        }

        prev_date = Some(row.date);
    }

    workbook
        .save(output_path)
        .with_context(|| format!("Не удалось сохранить {}", output_path.display()))?;
    Ok(())
}
