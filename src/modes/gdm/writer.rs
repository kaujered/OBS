//! Запись книги прогнозных таблиц.
//!
//! Книга повторяет структуру `шаблон.xlsx`: четыре скрытых листа-образца, лист
//! месторождения, листы площадей и поскважинные сводные листы.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;
use rayon::prelude::*;
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Workbook, Worksheet, XlsxError};

use crate::paths;

use super::GdmField;
use super::data::{Book, DopSheet, ExtraRow, Row, SectionSheet, WellRow};

const FILL_HEADER: u32 = 0xD9D9D9;
const FILL_SUBHEADER: u32 = 0xF2F2F2;

const MEST_TITLES: [&str; 7] = [
    "Дата",
    "Суточный отбор газа, млн. м3/сут",
    "Отбор газа за квартал, млрд. м3",
    "Накопл. отбор газа, млрд. м3",
    "Внедрение воды в залежь, млн. м3",
    "Среднее давление в залежи, МПа",
    "Фонд действ. скважин, шт.",
];
const MEST_CODES: [&str; 7] = [
    "DATE",
    "FGPR SM3/DAY *10**6",
    "",
    "FGPT SM3 *10**9",
    "AAQT SM3 *10**6 1",
    "FPR BARSA",
    "FMWPR",
];
const MEST_WIDTHS: [f64; 7] = [
    16.0,
    24.7109375,
    12.85546875,
    19.28515625,
    28.28515625,
    13.28515625,
    19.28515625,
];

const WELL_AVERAGES: &str = "Средние показатели работы действующих скважин";
const GROUP_SUBTITLES: [&str; 4] = [
    "Пластовое давление, МПа",
    "Устьевое давление, МПа",
    "Депрессия на пласт, МПа",
    "Средний дебит газа, тыс. м3/сут.",
];
const GROUP_CODES: [&str; 10] = [
    "DATE",
    "GGPR",
    "",
    "GGPT",
    "WBP BARSA *",
    "WTHP BARSA *",
    "WBHP BARSA *",
    "WGPR SM3/DAY *",
    "GMWPR",
    "GPR",
];
const GROUP_WIDTHS: [f64; 10] = [
    16.85546875,
    34.0,
    14.85546875,
    28.42578125,
    15.28515625,
    26.85546875,
    17.0,
    19.5703125,
    28.7109375,
    38.7109375,
];

const PLAST_SUBTITLES: [&str; 8] = [
    "Суточный отбор газа, млн. м3/сут",
    "Отбор газа за квартал, млрд. м3",
    "Накопл. отбор газа, млрд. м3",
    "Пластовое давление, МПа",
    "Устьевое давление, МПа",
    "Депрессия на пласт, МПа",
    "Средний дебит газа, тыс. м3/сут.",
    "Фонд действ. скважин, шт.",
];
const PLAST_CODES: [&str; 9] = [
    "DATE",
    "wgpr",
    "",
    "wgpt",
    "WBP BARSA *",
    "WTHP BARSA *",
    "WBHP BARSA *",
    "WGPR SM3/DAY *",
    "wgpr_count",
];

const DOP_TITLES: [&str; 4] = ["скважина", "Площадь", "Пласт", "Пласт_ГП"];
const DOP_WIDTHS: [f64; 4] = [14.57421875, 10.57421875, 11.00390625, 14.28125];

/// rust_xlsxwriter добавляет к ширине колонки внутренний отступ Excel, а в
/// шаблоне лежит уже готовое значение — компенсируем, чтобы книга совпадала с
/// образцом.
const WIDTH_PADDING: f64 = 0.7109375;

fn set_width(sheet: &mut Worksheet, column: u16, width: f64) -> Result<(), XlsxError> {
    sheet.set_column_width(column, (width - WIDTH_PADDING).max(0.0))?;
    Ok(())
}

/// Тип табличной части: от него зависит, какая колонка выводится целым числом.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    Mest,
    Group,
    Plast,
}

struct Formats {
    head: Format,
    head_num: Format,
    head_int: Format,
    head_num_flat: Format,
    head_num_left: Format,
    head_int_left: Format,
    sub: Format,
    sub_num: Format,
    sub_int: Format,
    sub_date: Format,
    sub_num_top: Format,
    dop_well: Format,
    dop_meta: Format,
    dop_date: Format,
    data_num: Format,
    data_int: Format,
    data_date: Format,
    block_title: Format,
}

impl Formats {
    fn new() -> Self {
        let head_base = || {
            Format::new()
                .set_font_name("Times New Roman")
                .set_font_size(11)
                .set_bold()
                .set_border(FormatBorder::Thin)
                .set_background_color(Color::RGB(FILL_HEADER))
        };
        let sub_base = || {
            Format::new()
                .set_font_name("Times New Roman")
                .set_font_size(11)
                .set_bold()
                .set_border(FormatBorder::Thin)
                .set_background_color(Color::RGB(FILL_SUBHEADER))
        };
        let centered = |format: Format| {
            format
                .set_align(FormatAlign::Center)
                .set_align(FormatAlign::VerticalCenter)
        };
        let data_base = || {
            Format::new()
                .set_border(FormatBorder::Thin)
                .set_align(FormatAlign::Center)
                .set_align(FormatAlign::VerticalCenter)
        };

        Self {
            head: centered(head_base()),
            head_num: centered(head_base()).set_text_wrap().set_num_format("0.00"),
            head_int: centered(head_base()).set_text_wrap().set_num_format("0"),
            head_num_flat: centered(head_base()).set_num_format("0.00"),
            head_num_left: head_base()
                .set_align(FormatAlign::VerticalCenter)
                .set_text_wrap()
                .set_num_format("0.00"),
            head_int_left: head_base()
                .set_align(FormatAlign::VerticalCenter)
                .set_text_wrap()
                .set_num_format("0"),
            sub: centered(sub_base()),
            sub_num: centered(sub_base()).set_text_wrap().set_num_format("0.00"),
            sub_int: centered(sub_base()).set_text_wrap().set_num_format("0"),
            sub_date: centered(sub_base()).set_num_format("dd/mm/yyyy"),
            sub_num_top: sub_base()
                .set_align(FormatAlign::Center)
                .set_text_wrap()
                .set_num_format("0.00"),
            dop_well: Format::new()
                .set_font_size(11)
                .set_bold()
                .set_border(FormatBorder::Thin),
            dop_meta: Format::new()
                .set_font_name("Arial Cyr")
                .set_font_size(10)
                .set_bold()
                .set_border(FormatBorder::Thin)
                .set_align(FormatAlign::Top)
                .set_num_format("0.00"),
            dop_date: data_base(),
            data_num: data_base().set_num_format("0.00"),
            data_int: data_base().set_num_format("0"),
            data_date: data_base().set_num_format("dd/mm/yyyy"),
            block_title: Format::new().set_font_size(14).set_bold(),
        }
    }
}

pub(super) fn write_workbook(field: GdmField, book: &Book, result_dir: &Path) -> Result<PathBuf> {
    let stamp = Local::now().format("%d.%m.%Y_%Hh%Mm%Ss");
    let path = result_dir.join(format!("qmain_{}_прогноз_{stamp}.xlsx", field.code()));
    paths::ensure_parent_dir(&path)?;

    build(book, &path).with_context(|| format!("Не удалось записать {}", path.display()))?;
    Ok(path)
}

fn build(book: &Book, path: &Path) -> Result<(), XlsxError> {
    let formats = Formats::new();
    let mut workbook = Workbook::new();

    // Листы-образцы остаются в книге скрытыми — так же, как их оставляет
    // исходный скрипт, копирующий из них шапки.
    for (name, kind) in [
        ("месторождение", Table::Mest),
        ("площадь", Table::Group),
        ("пласт", Table::Plast),
    ] {
        let sheet = workbook.add_worksheet();
        sheet.set_name(name)?;
        sheet.set_hidden(true);
        match kind {
            Table::Mest => write_mest_header(sheet, &formats)?,
            Table::Group => write_group_header(sheet, &formats)?,
            Table::Plast => {
                write_plast_header(sheet, &formats, 0, true)?;
                for (column, width) in GROUP_WIDTHS.iter().take(9).enumerate() {
                    set_width(sheet, column as u16, *width)?;
                }
            }
        }
    }
    let dop_template = workbook.add_worksheet();
    dop_template.set_name("dop")?;
    dop_template.set_hidden(true);
    write_dop_header(dop_template, &formats, &[])?;

    // Поскважинные листы — это миллионы ячеек, поэтому листы наполняются
    // параллельно как самостоятельные `Worksheet`, а в книгу попадают уже
    // готовыми: порядок задаёт порядок `jobs`.
    let jobs: Vec<SheetJob<'_>> = std::iter::once(SheetJob::Section(&book.field, Table::Mest))
        .chain(
            book.groups
                .iter()
                .map(|group| SheetJob::Section(group, Table::Group)),
        )
        .chain(book.dop.iter().map(SheetJob::Dop))
        .collect();

    let sheets = jobs
        .into_par_iter()
        .map(|job| {
            let mut sheet = Worksheet::new();
            match job {
                SheetJob::Section(section, table) => {
                    write_section(&mut sheet, &formats, section, table)?;
                }
                SheetJob::Dop(dop) => {
                    sheet.set_name(dop.name)?;
                    write_dop_header(&mut sheet, &formats, &dop.dates)?;
                    write_dop_body(&mut sheet, &formats, &dop.wells, &dop.extra)?;
                }
            }
            Ok(sheet)
        })
        .collect::<Result<Vec<_>, XlsxError>>()?;

    for (index, mut sheet) in sheets.into_iter().enumerate() {
        // Активным остаётся лист месторождения — первый видимый лист книги.
        if index == 0 {
            sheet.set_active(true);
        }
        workbook.push_worksheet(sheet);
    }

    // `Workbook::save` пишет в файл без буфера, а упаковщик отдаёт архив
    // мелкими кусками, поэтому книга на 60 МБ уходит десятками тысяч системных
    // вызовов. Буфер на мегабайт убирает их.
    let file = BufWriter::with_capacity(1 << 20, File::create(path)?);
    workbook.save_to_writer(file)?;
    Ok(())
}

/// Лист, который наполняется в отдельном потоке.
enum SheetJob<'a> {
    Section(&'a SectionSheet, Table),
    Dop(&'a DopSheet),
}

fn write_section(
    sheet: &mut Worksheet,
    formats: &Formats,
    section: &SectionSheet,
    table: Table,
) -> Result<(), XlsxError> {
    sheet.set_name(&section.title)?;
    let mut next = match table {
        Table::Mest => {
            write_mest_header(sheet, formats)?;
            2
        }
        Table::Group => {
            write_group_header(sheet, formats)?;
            3
        }
        Table::Plast => unreachable!("лист пласта отдельно не создаётся"),
    };
    next = write_rows(sheet, formats, next, &section.rows, table)?;

    for (title, rows) in &section.blocks {
        // Заголовок блока отбивается двумя пустыми строками, как в исходнике.
        let title_row = next + 2;
        sheet.write_string_with_format(title_row, 0, title.as_str(), &formats.block_title)?;
        write_plast_header(sheet, formats, title_row + 1, false)?;
        next = write_rows(sheet, formats, title_row + 4, rows, Table::Plast)?;
    }

    Ok(())
}

fn write_rows(
    sheet: &mut Worksheet,
    formats: &Formats,
    first_row: u32,
    rows: &[Row],
    table: Table,
) -> Result<u32, XlsxError> {
    for (offset, row) in rows.iter().enumerate() {
        let index = first_row + offset as u32;
        sheet.write_string_with_format(index, 0, row.date.as_ref(), &formats.data_date)?;
        for (column, value) in row.values.iter().enumerate() {
            let column = column as u16 + 1;
            let format = data_format(formats, table, column + 1);
            match value {
                Some(value) => sheet.write_number_with_format(index, column, *value, format)?,
                None => sheet.write_blank(index, column, format)?,
            };
        }
    }
    Ok(first_row + rows.len() as u32)
}

/// Целочисленный формат стоит на «Фонде действующих скважин»: в таблице
/// месторождения это седьмая колонка, в остальных — девятая.
fn data_format(formats: &Formats, table: Table, column_1based: u16) -> &Format {
    match table {
        Table::Mest if column_1based == 7 => &formats.data_int,
        Table::Group | Table::Plast if column_1based == 9 => &formats.data_int,
        _ => &formats.data_num,
    }
}

fn write_mest_header(sheet: &mut Worksheet, formats: &Formats) -> Result<(), XlsxError> {
    for (column, (title, code)) in MEST_TITLES.iter().zip(MEST_CODES).enumerate() {
        let column = column as u16;
        let (top, bottom) = match column {
            0 => (&formats.head, &formats.sub),
            6 => (&formats.head_int, &formats.sub_int),
            _ => (&formats.head_num, &formats.sub_num),
        };
        sheet.write_string_with_format(0, column, *title, top)?;
        sheet.write_string_with_format(1, column, code, bottom)?;
        set_width(sheet, column, MEST_WIDTHS[column as usize])?;
    }
    sheet.set_row_height(0, 42.75)?;
    sheet.set_row_height(1, 14.25)?;
    Ok(())
}

fn write_group_header(sheet: &mut Worksheet, formats: &Formats) -> Result<(), XlsxError> {
    for (column, title) in MEST_TITLES.iter().take(4).enumerate() {
        sheet.merge_range(0, column as u16, 1, column as u16, title, &formats.head_num)?;
    }
    sheet.merge_range(0, 4, 0, 7, WELL_AVERAGES, &formats.head_num_flat)?;
    sheet.merge_range(0, 8, 1, 8, MEST_TITLES[6], &formats.head_int)?;
    sheet.merge_range(0, 9, 1, 9, "Давление входа в УКПГ, МПа", &formats.head_num)?;
    for (offset, title) in GROUP_SUBTITLES.iter().enumerate() {
        sheet.write_string_with_format(1, offset as u16 + 4, *title, &formats.head_num_left)?;
    }
    for (column, code) in GROUP_CODES.iter().enumerate() {
        let column = column as u16;
        let format = match column {
            0 => &formats.sub_date,
            8 => &formats.sub_int,
            9 => &formats.sub_num_top,
            _ => &formats.sub_num,
        };
        sheet.write_string_with_format(2, column, *code, format)?;
        set_width(sheet, column, GROUP_WIDTHS[column as usize])?;
    }
    sheet.set_row_height(1, 28.5)?;
    Ok(())
}

/// Шапка блока по пласту: три строки, начиная с `first_row`.
fn write_plast_header(
    sheet: &mut Worksheet,
    formats: &Formats,
    first_row: u32,
    set_row_height: bool,
) -> Result<(), XlsxError> {
    sheet.merge_range(
        first_row,
        0,
        first_row + 1,
        0,
        MEST_TITLES[0],
        &formats.head_num,
    )?;
    sheet.merge_range(
        first_row,
        1,
        first_row,
        8,
        WELL_AVERAGES,
        &formats.head_num_flat,
    )?;
    for (offset, title) in PLAST_SUBTITLES.iter().enumerate() {
        let column = offset as u16 + 1;
        let format = if column == 8 {
            &formats.head_int_left
        } else {
            &formats.head_num_left
        };
        sheet.write_string_with_format(first_row + 1, column, *title, format)?;
    }
    for (column, code) in PLAST_CODES.iter().enumerate() {
        let column = column as u16;
        let format = match column {
            0 => &formats.sub_date,
            8 => &formats.sub_int,
            _ => &formats.sub_num,
        };
        sheet.write_string_with_format(first_row + 2, column, *code, format)?;
    }
    if set_row_height {
        sheet.set_row_height(first_row + 1, 42.75)?;
    }
    Ok(())
}

fn write_dop_header(
    sheet: &mut Worksheet,
    formats: &Formats,
    dates: &[Box<str>],
) -> Result<(), XlsxError> {
    for (column, title) in DOP_TITLES.iter().enumerate() {
        let format = if column == 0 {
            &formats.dop_well
        } else {
            &formats.dop_meta
        };
        sheet.write_string_with_format(0, column as u16, *title, format)?;
        set_width(sheet, column as u16, DOP_WIDTHS[column])?;
    }
    for (offset, date) in dates.iter().enumerate() {
        let column = offset as u16 + 4;
        sheet.write_string_with_format(0, column, date.as_ref(), &formats.dop_date)?;
        set_width(sheet, column, 10.0)?;
    }
    Ok(())
}

fn write_dop_body(
    sheet: &mut Worksheet,
    formats: &Formats,
    wells: &[WellRow],
    extra: &[ExtraRow],
) -> Result<(), XlsxError> {
    for (offset, well) in wells.iter().enumerate() {
        let row = offset as u32 + 1;
        match well.well_number {
            Some(number) => {
                sheet.write_number_with_format(row, 0, number, &formats.data_int)?;
            }
            None => {
                sheet.write_string_with_format(
                    row,
                    0,
                    well.well_text.as_ref(),
                    &formats.data_int,
                )?;
            }
        };
        sheet.write_string_with_format(row, 1, well.gp.as_ref(), &formats.data_num)?;
        sheet.write_string_with_format(row, 2, well.plast.as_ref(), &formats.data_num)?;
        sheet.write_string_with_format(row, 3, well.plast_gp.as_ref(), &formats.data_num)?;
        write_values(sheet, formats, row, &well.values)?;
    }

    let first_extra = wells.len() as u32 + 3;
    for (offset, row_data) in extra.iter().enumerate() {
        let row = first_extra + offset as u32;
        sheet.write_string_with_format(row, 0, row_data.label.as_ref(), &formats.data_int)?;
        sheet.write_string_with_format(row, 1, row_data.kind, &formats.data_num)?;
        sheet.write_blank(row, 2, &formats.data_num)?;
        sheet.write_blank(row, 3, &formats.data_num)?;
        write_values(sheet, formats, row, &row_data.values)?;
    }

    Ok(())
}

fn write_values(
    sheet: &mut Worksheet,
    formats: &Formats,
    row: u32,
    values: &[Option<f64>],
) -> Result<(), XlsxError> {
    for (offset, value) in values.iter().enumerate() {
        let column = offset as u16 + 4;
        match value {
            Some(value) => {
                sheet.write_number_with_format(row, column, *value, &formats.data_num)?;
            }
            None => {
                sheet.write_blank(row, column, &formats.data_num)?;
            }
        };
    }
    Ok(())
}
