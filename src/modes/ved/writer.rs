//! Excel template filling helpers for `VED`.

use super::*;

use std::io::{Cursor, Read, Seek, Write};

use ahash::AHashMap as HashMap;

use super::sheet_xml::{SheetXml, add_yellow_styles, parse_shared_strings};

/// Параметры заполнения одного шаблона.
pub(super) struct FillOptions<'a> {
    pub(super) regime: Option<&'a RegimeData>,
    pub(super) pressure_conversion: bool,
    pub(super) correct_work_params: bool,
}

pub(super) fn fill_template(
    template_path: &Path,
    output_dir: &Path,
    combined_map: &HashMap<i64, CombinedRow>,
    options: &FillOptions<'_>,
    report_year: i32,
    report_month: u32,
) -> Result<PathBuf> {
    let replacement_text = target_period_title(report_year, report_month)?;
    let output_suffix = target_period_label(report_year, report_month)?;

    let template_bytes = std::fs::read(template_path)
        .with_context(|| format!("Не удалось открыть шаблон {}", template_path.display()))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(template_bytes.as_slice()))
        .with_context(|| format!("Некорректный шаблон {}", template_path.display()))?;

    let workbook_xml = read_zip_entry(&mut archive, "xl/workbook.xml")?
        .ok_or_else(|| anyhow!("Нет xl/workbook.xml в {}", template_path.display()))?;
    let sheet_paths = workbook_sheet_paths(&mut archive)?;
    let sheet_name = target_sheet_name(&workbook_xml, &sheet_paths)?;
    let sheet_path = sheet_paths
        .get(&sheet_name)
        .cloned()
        .ok_or_else(|| anyhow!("Не найден лист {sheet_name} в {}", template_path.display()))?;

    let shared = match read_zip_entry(&mut archive, "xl/sharedStrings.xml")? {
        Some(xml) => parse_shared_strings(&xml),
        None => Vec::new(),
    };
    let sheet_source = read_zip_entry(&mut archive, &sheet_path)?
        .ok_or_else(|| anyhow!("Нет листа {sheet_path} в {}", template_path.display()))?;
    let mut sheet = SheetXml::parse(sheet_source, &shared)
        .with_context(|| format!("Не удалось разобрать лист {}", template_path.display()))?;

    replace_title(&mut sheet, &replacement_text);
    let (row_well, row_main) = find_header_rows(&sheet)?;
    let col_map = build_column_map(&sheet, row_well, row_main);
    let col_well = col_map
        .get("well")
        .copied()
        .ok_or_else(|| anyhow!("Не найдена колонка '№ скв' в шаблоне"))?;
    let data_start = find_data_start_row(&sheet, col_well, row_main + 1);
    fill_sheet_rows(
        &mut sheet,
        &col_map,
        col_well,
        data_start,
        combined_map,
        options,
    );
    fill_rvh_cells(&mut sheet, &col_map, options.regime);

    // Производные стили с жёлтой заливкой для помеченных ячеек.
    let yellow_sources = sheet.yellow_source_styles();
    let (styles_xml, yellow_map) = if yellow_sources.is_empty() {
        (None, HashMap::default())
    } else {
        let source = read_zip_entry(&mut archive, "xl/styles.xml")?
            .ok_or_else(|| anyhow!("Нет xl/styles.xml в {}", template_path.display()))?;
        let (xml, map) = add_yellow_styles(&source, &yellow_sources)?;
        (Some(xml), map)
    };
    let filled_sheet_xml = sheet.render(&yellow_map)?;

    let stem = template_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("Некорректное имя шаблона {}", template_path.display()))?;
    let base_stem = OUTPUT_SUFFIX_RE.replace(stem, "").to_string();
    let output_path = output_dir.join(format!("{base_stem}_на_{output_suffix}.xlsx"));
    paths::ensure_parent_dir(&output_path)?;
    let final_bytes = assemble_workbook(&mut archive, &sheet_path, &filled_sheet_xml, styles_xml)
        .with_context(|| format!("Не удалось собрать {}", output_path.display()))?;
    std::fs::write(&output_path, final_bytes)
        .with_context(|| format!("Не удалось сохранить {}", output_path.display()))?;
    Ok(output_path)
}

/// Целевой лист заполнения: «Лист1», если есть, иначе активный лист книги.
fn target_sheet_name(workbook_xml: &str, sheet_paths: &HashMap<String, String>) -> Result<String> {
    if sheet_paths.contains_key("Лист1") {
        return Ok("Лист1".to_string());
    }
    static ACTIVE_TAB_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"activeTab="(\d+)""#).expect("valid activeTab regex"));
    let names: Vec<&str> = SHEET_TAG_RE
        .find_iter(workbook_xml)
        .filter_map(|tag| tag_attr(tag.as_str(), "name"))
        .collect();
    let active: usize = ACTIVE_TAB_RE
        .captures(workbook_xml)
        .and_then(|caps| caps[1].parse().ok())
        .unwrap_or(0);
    names
        .get(active)
        .or_else(|| names.first())
        .map(|name| (*name).to_string())
        .ok_or_else(|| anyhow!("В книге нет листов"))
}

/// Сборка выходного архива: пересжимаются только изменённые записи
/// (заполненный лист, styles.xml при жёлтых заливках, workbook.xml и листы
/// с очищенным кэшем формул), остальное копируется без перепаковки.
/// Кэш формул удаляется, а в calcPr включается fullCalcOnLoad, чтобы
/// Excel/OnlyOffice пересчитали формулы при открытии; условное
/// форматирование из `<extLst>` шаблона сохраняется как есть.
fn assemble_workbook<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    filled_sheet_path: &str,
    filled_sheet_xml: &str,
    styles_xml: Option<String>,
) -> Result<Vec<u8>> {
    let mut buffer = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        let is_worksheet = name.starts_with("xl/worksheets/") && name.ends_with(".xml");
        let patched: Option<String> = if name == filled_sheet_path {
            Some(
                FORMULA_CACHE_RE
                    .replace_all(filled_sheet_xml, "$1")
                    .into_owned(),
            )
        } else if is_worksheet {
            let mut xml = String::new();
            entry.read_to_string(&mut xml)?;
            Some(FORMULA_CACHE_RE.replace_all(&xml, "$1").into_owned())
        } else if name == "xl/workbook.xml" {
            let mut xml = String::new();
            entry.read_to_string(&mut xml)?;
            if !xml.contains("fullCalcOnLoad") {
                if xml.contains("<calcPr ") {
                    xml = xml.replace("<calcPr ", "<calcPr fullCalcOnLoad=\"1\" ");
                } else if let Some(pos) = xml
                    .find("</definedNames>")
                    .map(|found| found + "</definedNames>".len())
                    .or_else(|| xml.find("</sheets>").map(|found| found + "</sheets>".len()))
                {
                    // в шаблоне может не быть calcPr вовсе; по схеме он идёт
                    // после definedNames/sheets
                    xml.insert_str(pos, "<calcPr fullCalcOnLoad=\"1\"/>");
                }
            }
            Some(xml)
        } else if name == "xl/styles.xml" && styles_xml.is_some() {
            styles_xml.clone()
        } else {
            None
        };
        match patched {
            Some(xml) => {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(xml.as_bytes())?;
            }
            None => writer.raw_copy_file(entry)?,
        }
    }
    writer.finish()?;
    Ok(buffer.into_inner())
}

fn replace_title(sheet: &mut SheetXml, replacement_text: &str) {
    let target = sheet.merge_starts().iter().copied().find(|&(row, col)| {
        sheet
            .cell_text(row, col)
            .to_lowercase()
            .contains(TITLE_SEARCH_LC.as_str())
    });

    if let Some((row, col)) = target {
        sheet.set_text(row, col, replacement_text.to_string());
    }
}

fn find_header_rows(sheet: &SheetXml) -> Result<(u32, u32)> {
    let max_row = sheet.max_row().min(250);
    let max_col = sheet.max_col().min(80);
    let mut row_well = None;
    let mut row_main = None;

    for row in 1..=max_row {
        for col in 1..=max_col {
            let text = sheet.cell_text(row, col).to_lowercase();
            if row_well.is_none()
                && (text.contains("№ скв") || (text.contains("скв") && text.contains("в„–")))
            {
                row_well = Some(row);
            }
            if row_main.is_none() && text.contains("дата замера") {
                row_main = Some(row);
            }
        }

        if row_well.is_some() && row_main.is_some() {
            break;
        }
    }

    match (row_well, row_main) {
        (Some(row_well), Some(row_main)) => Ok((row_well, row_main)),
        _ => bail!("Не удалось найти заголовки '№ скв.' и/или 'Дата замера' в шаблоне."),
    }
}

fn build_column_map(sheet: &SheetXml, row_well: u32, row_main: u32) -> HashMap<&'static str, u32> {
    let header_rows = [row_well, row_main, row_main + 1, row_main + 2, row_main + 3];
    let header_texts = collect_lowercase_header_rows(sheet, &header_rows, sheet.max_col().min(220));
    let mut col_map = HashMap::with_capacity(14);

    insert_column(
        &mut col_map,
        "well",
        find_column_by_predicate(&header_texts, &[row_well], |text| text.contains("скв")),
    );

    // Колонки, которые ищутся по одной подстроке в любой из строк шапки;
    // «Т ус. (С)» и «Т гидр. (С)» есть только в шаблоне Ныды.
    const SUBSTRING_COLUMNS: [(&str, &str); 14] = [
        ("date_meas", "дата замера"),
        ("rst", "рст."),
        ("rpl", "рпл."),
        ("depr", "депр"),
        ("rus", "рус"),
        ("rzt", "рзт"),
        ("rshl", "ршл"),
        ("tus", "тус"),
        ("rmk", "рмк"),
        ("rvh", "рвх"),
        ("tvh", "твх"),
        ("qwat", "воды"),
        ("dop_tus", "т ус"),
        ("dop_tgidr", "т гидр"),
    ];
    for (key, needle) in SUBSTRING_COLUMNS {
        insert_column(
            &mut col_map,
            key,
            find_column_by_predicate(&header_texts, &header_rows, |text| text.contains(needle)),
        );
    }
    insert_column(
        &mut col_map,
        "q_work",
        find_column_by_predicate(&header_texts, &header_rows, |text| {
            text.contains("рабоч") && text.contains("дебит")
        }),
    );

    let qdop = find_last_column_by_predicate(&header_texts, &header_rows, |text| {
        text.contains('q')
            && text.contains("тыс")
            && (text.contains("м3/сут")
                || text.contains("м3")
                || text.contains("м³/сут")
                || text.contains("м³"))
    });
    insert_column(&mut col_map, "qdop", qdop);

    // Блоки «Технологического режима»: объединённая шапка занимает три
    // колонки в фиксированном порядке Рус./Депр./Q.
    let regime_blocks = [
        (["opt_rus", "opt_depr", "opt_q"], "оптимальный режим"),
        (
            ["dop_rus", "dop_depr", "dop_q"],
            "допустимый режим промысла",
        ),
    ];
    for (keys, needle) in regime_blocks {
        if let Some(start) =
            find_column_by_predicate(&header_texts, &header_rows, |text| text.contains(needle))
        {
            for (offset, key) in keys.into_iter().enumerate() {
                col_map.insert(key, start + offset as u32);
            }
        }
    }

    col_map
}

fn insert_column(map: &mut HashMap<&'static str, u32>, key: &'static str, value: Option<u32>) {
    if let Some(value) = value {
        map.insert(key, value);
    }
}

type LowercaseHeaderRows = Vec<(u32, Vec<String>)>;

fn collect_lowercase_header_rows(
    sheet: &SheetXml,
    rows: &[u32],
    max_col: u32,
) -> LowercaseHeaderRows {
    rows.iter()
        .copied()
        .map(|row| {
            let texts = (1..=max_col)
                .map(|col| sheet.cell_text(row, col).to_lowercase())
                .collect();
            (row, texts)
        })
        .collect()
}

fn row_header_texts(header_texts: &LowercaseHeaderRows, row: u32) -> Option<&[String]> {
    header_texts
        .iter()
        .find(|(cached_row, _)| *cached_row == row)
        .map(|(_, texts)| texts.as_slice())
}

fn find_column_by_predicate<F>(
    header_texts: &LowercaseHeaderRows,
    rows: &[u32],
    predicate: F,
) -> Option<u32>
where
    F: Fn(&str) -> bool,
{
    for row in rows {
        let Some(texts) = row_header_texts(header_texts, *row) else {
            continue;
        };
        for (index, text) in texts.iter().enumerate() {
            if !text.is_empty() && predicate(text) {
                return Some((index + 1) as u32);
            }
        }
    }
    None
}

fn find_last_column_by_predicate<F>(
    header_texts: &LowercaseHeaderRows,
    rows: &[u32],
    predicate: F,
) -> Option<u32>
where
    F: Fn(&str) -> bool,
{
    let mut found = None;
    for row in rows {
        let Some(texts) = row_header_texts(header_texts, *row) else {
            continue;
        };
        for (index, text) in texts.iter().enumerate() {
            if !text.is_empty() && predicate(text) {
                found = Some((index + 1) as u32);
            }
        }
    }
    found
}

fn find_data_start_row(sheet: &SheetXml, col_well: u32, row_after_headers: u32) -> u32 {
    for row in row_after_headers..=sheet.max_row() {
        if normalize_well_text(sheet.cell_text(row, col_well)).is_some() {
            return row;
        }
    }
    row_after_headers
}

fn fill_sheet_rows(
    sheet: &mut SheetXml,
    col_map: &HashMap<&'static str, u32>,
    col_well: u32,
    data_start: u32,
    combined_map: &HashMap<i64, CombinedRow>,
    options: &FillOptions<'_>,
) {
    let regime = options.regime;
    // 0.1 ата в единицах шаблона: МПа-шаблоны получают пересчитанный шаг.
    let pressure_step = if options.pressure_conversion {
        0.1 / KGS_CM2_TO_MPA
    } else {
        0.1
    };
    let mut empty_run = 0u32;
    for row in data_start..=sheet.max_row() {
        let well = normalize_well_text(sheet.cell_text(row, col_well));
        let Some(well) = well else {
            empty_run += 1;
            if empty_run >= 30 {
                break;
            }
            continue;
        };
        empty_run = 0;

        let source = combined_map.get(&well);
        let regime_row = regime.and_then(|data| data.rows.get(&well));
        if source.is_none() && regime_row.is_none() {
            continue;
        }

        // Рст всегда округляется до трёх знаков — и для ячейки, и для расчётов
        let rst = source
            .and_then(|row| row.rst)
            .map(|value| round_to(value, 3));
        let blocks = prepare_regime_blocks(regime_row, rst, pressure_step);

        if let Some(source) = source {
            write_source_cells(
                sheet,
                col_map,
                row,
                source,
                rst,
                &blocks,
                options.correct_work_params,
            );
        }

        // Допустимый по скважине: из режимного листа (Qдоп.скв.), и только
        // без режимных листов — из СкважиныГДН, как раньше.
        let qdop = match regime {
            Some(_) => regime_row.and_then(|row| row.qdop_skv),
            None => source.and_then(|row| row.qdop),
        };
        sheet.set_num_if_allowed(row, col_map.get("qdop").copied(), qdop);

        if let Some(regime_row) = regime_row {
            write_regime_cells(sheet, col_map, row, regime_row, &blocks);
        }
    }
}

/// Параметры режима (оптимального или допустимого) в единицах шаблона.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct RegimeParams {
    rus: Option<f64>,
    depr: Option<f64>,
    q: Option<f64>,
}

/// Пригодная для аппроксимации точка режима.
#[derive(Debug, Clone, Copy)]
struct RegimePoint {
    q: f64,
    depr: f64,
    rus: f64,
}

impl RegimeParams {
    /// Округление для внесения в ведомость и аппроксимации:
    /// Q — до целых, Рус и Депр — до трёх знаков после запятой.
    fn rounded(self) -> Self {
        Self {
            rus: self.rus.map(|value| round_to(value, 3)),
            depr: self.depr.map(|value| round_to(value, 3)),
            q: self.q.map(f64::round),
        }
    }

    /// Остановленный режим (нулевой дебит): Рус прибит к Рст.
    fn is_stopped(self) -> bool {
        self.q.is_some_and(|value| value.abs() < f64::EPSILON)
    }

    /// Пригодная для аппроксимации точка: дебит положительный и хотя бы
    /// одно из давлений ненулевое.
    fn as_point(self) -> Option<RegimePoint> {
        let (Some(rus), Some(depr), Some(q)) = (self.rus, self.depr, self.q) else {
            return None;
        };
        (q > 0.0 && (rus > 0.0 || depr > 0.0)).then_some(RegimePoint { q, depr, rus })
    }
}

/// Блоки техрежима строки после всех подстановок и округлений; исходные Рус
/// сохраняются для подсветки значений, заменённых подстановкой Рст - Депр.
struct RegimeBlocks {
    opt: Option<RegimeParams>,
    dop: Option<RegimeParams>,
    raw_opt_rus: Option<f64>,
    raw_dop_rus: Option<f64>,
}

/// Значения блоков техрежима с уже применёнными подстановками
/// (Рус = Рст при нулевом дебите, 0.1 ата при нулевых давлениях):
/// именно они отображаются, поэтому и согласование ведём с ними.
fn prepare_regime_blocks(
    regime_row: Option<&RegimeRow>,
    rst: Option<f64>,
    pressure_step: f64,
) -> RegimeBlocks {
    let opt = regime_row.map(|row| {
        regime_block(row.opt_q, row.opt_rus, row.opt_depr, rst, pressure_step).rounded()
    });
    let dop = regime_row.map(|row| {
        regime_block(row.dop_q, row.dop_rus, row.dop_depr, rst, pressure_step).rounded()
    });
    let (opt, dop) = unify_equal_regimes(opt, dop);
    let raw_opt_rus = opt.and_then(|params| params.rus);
    let raw_dop_rus = dop.and_then(|params| params.rus);
    let (opt, dop) = cap_regime_rus_to_rst(rst, opt, dop);
    // Рус, подставленный как Рст - Депр, тоже округляется
    RegimeBlocks {
        opt: opt.map(RegimeParams::rounded),
        dop: dop.map(RegimeParams::rounded),
        raw_opt_rus,
        raw_dop_rus,
    }
}

/// Ячейки «Геолого-промысловых данных» строки: замеры из сводки и
/// согласованные (при включённой коррекции) рабочие Депр/Рус.
#[allow(clippy::too_many_arguments)]
fn write_source_cells(
    sheet: &mut SheetXml,
    col_map: &HashMap<&'static str, u32>,
    row: u32,
    source: &CombinedRow,
    rst: Option<f64>,
    blocks: &RegimeBlocks,
    correct_work_params: bool,
) {
    let (work_rus, work_depr) = if correct_work_params {
        correct_work_pressure(
            source.q_work,
            source.rus,
            source.depr,
            rst,
            blocks.opt,
            blocks.dop,
        )
    } else {
        (source.rus, source.depr)
    };
    sheet.set_date_if_allowed(row, col_map.get("date_meas").copied(), source.date_key);
    let values = [
        ("rst", rst),
        ("rpl", source.rpl.map(|value| round_to(value, 3))),
        ("q_work", source.q_work),
        ("depr", work_depr),
        ("rus", work_rus),
        ("rzt", source.rzt),
        ("rshl", source.rshl),
        ("tus", source.tus),
        ("rmk", source.rmk),
        ("rvh", source.rvh),
        ("tvh", source.tvh),
        ("qwat", source.qwat),
    ];
    for (key, value) in values {
        sheet.set_num_if_allowed(row, col_map.get(key).copied(), value);
    }
}

/// Ячейки блоков «Технологический режим» строки с жёлтой подсветкой Рус,
/// заменённых подстановкой Рст - Депр.
fn write_regime_cells(
    sheet: &mut SheetXml,
    col_map: &HashMap<&'static str, u32>,
    row: u32,
    regime_row: &RegimeRow,
    blocks: &RegimeBlocks,
) {
    let opt = blocks.opt.unwrap_or_default();
    let dop = blocks.dop.unwrap_or_default();
    let values = [
        ("opt_rus", opt.rus),
        ("opt_depr", opt.depr),
        ("opt_q", opt.q),
        ("dop_rus", dop.rus),
        ("dop_depr", dop.depr),
        ("dop_q", dop.q),
        ("dop_tus", regime_row.dop_tus),
        ("dop_tgidr", regime_row.dop_tgidr),
    ];
    for (key, value) in values {
        sheet.set_num_if_allowed(row, col_map.get(key).copied(), value);
    }
    if opt.rus != blocks.raw_opt_rus {
        sheet.mark_cell_yellow(row, col_map.get("opt_rus").copied());
    }
    if dop.rus != blocks.raw_dop_rus {
        sheet.mark_cell_yellow(row, col_map.get("dop_rus").copied());
    }
}

fn round_to(value: f64, digits: i32) -> f64 {
    let factor = 10f64.powi(digits);
    (value * factor).round() / factor
}

/// Совпавшие после округления дебиты оптимального и допустимого режимов
/// означают один режим: Рус и Депр оптимального приравниваются к допустимому.
fn unify_equal_regimes(
    opt: Option<RegimeParams>,
    dop: Option<RegimeParams>,
) -> (Option<RegimeParams>, Option<RegimeParams>) {
    if let (Some(opt), Some(dop)) = (opt, dop)
        && let (Some(opt_q), Some(dop_q)) = (opt.q, dop.q)
        && (opt_q - dop_q).abs() < f64::EPSILON
    {
        return (Some(RegimeParams { q: opt.q, ..dop }), Some(dop));
    }
    (opt, dop)
}

/// Если на оптимальном или допустимом режиме Рус выше Рст (противоречивые
/// данные), Рус всех режимов заменяется на Рст - Депр соответствующего
/// режима; аппроксимация рабочих параметров идёт уже от новых значений.
/// Режимы с нулевым дебитом не учитываются и не трогаются: их Рус прибит к
/// Рст и после округления может оказаться чуть выше Рст, это не
/// противоречие данных.
fn cap_regime_rus_to_rst(
    rst: Option<f64>,
    opt: Option<RegimeParams>,
    dop: Option<RegimeParams>,
) -> (Option<RegimeParams>, Option<RegimeParams>) {
    let Some(rst) = rst else {
        return (opt, dop);
    };
    let exceeds = [opt, dop]
        .into_iter()
        .flatten()
        .any(|params| !params.is_stopped() && params.rus.is_some_and(|rus| rus > rst));
    if !exceeds {
        return (opt, dop);
    }
    let fix = |params: Option<RegimeParams>| {
        params.map(|params| match params.depr {
            Some(depr) if !params.is_stopped() => RegimeParams {
                rus: Some(rst - depr),
                ..params
            },
            _ => params,
        })
    };
    (fix(opt), fix(dop))
}

/// Аппроксимирует Депр и Рус линейной зависимостью от рабочего дебита.
/// Сам рабочий дебит не меняется:
/// - Q = 0 -> Рус = Рст, депрессия 0;
/// - дебит равен допустимому или оптимальному — параметры этого режима
///   берутся как есть;
/// - дебит выше допустимого — прямая через статику (0 для депрессии,
///   Рст для Рус) и точку допустимого режима;
/// - дебит ниже оптимального — прямая через статику и точку оптимального;
/// - дебит между оптимальным и допустимым — прямая через обе режимные
///   точки, без статики;
/// - при совпадающих режимах — прямая через статику и допустимый;
/// - без Рст в прямых от статики замеренный Рус остаётся;
/// - без пригодных режимных точек (нет дебита или оба давления нулевые)
///   значения из сводки остаются как есть.
fn correct_work_pressure(
    q_work: Option<f64>,
    rus: Option<f64>,
    depr: Option<f64>,
    rst: Option<f64>,
    opt: Option<RegimeParams>,
    dop: Option<RegimeParams>,
) -> (Option<f64>, Option<f64>) {
    let Some(q) = q_work else {
        return (rus, depr);
    };

    if q.abs() < f64::EPSILON {
        return match rst {
            Some(rst) => (Some(rst), Some(0.0)),
            None => (rus, depr),
        };
    }

    let opt_point = opt.and_then(RegimeParams::as_point);
    let dop_point = dop.and_then(RegimeParams::as_point);

    // Рабочий дебит (неокруглённый) совпал с режимным: рабочие параметры
    // равны параметрам этого режима, без аппроксимации.
    for point in [dop_point, opt_point].into_iter().flatten() {
        if (q - point.q).abs() < f64::EPSILON {
            return (Some(point.rus), Some(point.depr));
        }
    }

    // (нижняя точка для прямой между режимами, режимная точка)
    let (lower, point) = match (opt_point, dop_point) {
        (None, None) => return (rus, depr),
        (Some(point), None) | (None, Some(point)) => (None, point),
        (Some(opt), Some(dop)) => {
            if (dop.q - opt.q).abs() < f64::EPSILON || q > dop.q {
                (None, dop)
            } else if q < opt.q {
                (None, opt)
            } else {
                (Some(opt), dop)
            }
        }
    };

    match lower {
        // прямая через оптимальную и допустимую точки
        Some(opt) => (
            Some(line_value((opt.q, opt.rus), (point.q, point.rus), q).max(0.0)),
            Some(line_value((opt.q, opt.depr), (point.q, point.depr), q).max(0.0)),
        ),
        // прямая через статику и режимную точку
        None => (
            match rst {
                Some(rst) => Some(line_value((0.0, rst), (point.q, point.rus), q).max(0.0)),
                None => rus,
            },
            Some(line_value((0.0, 0.0), (point.q, point.depr), q).max(0.0)),
        ),
    }
}

/// Значение в точке q на прямой через две точки (дебит, значение).
fn line_value(first: (f64, f64), second: (f64, f64), q: f64) -> f64 {
    first.1 + (second.1 - first.1) / (second.0 - first.0) * (q - first.0)
}

/// Блок техрежима (оптимальный или допустимый) для отображения:
/// при нулевом дебите Рус равен Рст, иначе — подстановка 0.1 ата
/// при нулевых давлениях работающей скважины.
fn regime_block(
    q: Option<f64>,
    rus: Option<f64>,
    depr: Option<f64>,
    rst: Option<f64>,
    pressure_step: f64,
) -> RegimeParams {
    if q.is_some_and(|value| value.abs() < f64::EPSILON)
        && let Some(rst) = rst
    {
        return RegimeParams {
            rus: Some(rst),
            depr,
            q,
        };
    }
    let (rus, depr) = fallback_zero_pressure(q, rus, depr, rst, pressure_step);
    RegimeParams { rus, depr, q }
}

/// Скважина в работе (Q > 0), но давления в режимном листе нулевые:
/// подставляем депрессию 0.1 ата и Рус = Рст - 0.1 ата (в единицах шаблона).
fn fallback_zero_pressure(
    q: Option<f64>,
    rus: Option<f64>,
    depr: Option<f64>,
    rst: Option<f64>,
    pressure_step: f64,
) -> (Option<f64>, Option<f64>) {
    let needs_fallback = q.is_some_and(|value| value > 0.0)
        && rus.is_some_and(|value| value.abs() < f64::EPSILON)
        && depr.is_some_and(|value| value.abs() < f64::EPSILON);
    match (needs_fallback, rst) {
        (true, Some(rst)) => (Some(rst - pressure_step), Some(pressure_step)),
        _ => (rus, depr),
    }
}

/// Подставляет Рвх в ячейки вида «Рвх = x,x МПа» под блоками оптимального и
/// допустимого режима. При нескольких режимных листах (БНГКМ ГП2) плейсхолдеры
/// заполняются по порядку следования секций в шаблоне.
fn fill_rvh_cells(
    sheet: &mut SheetXml,
    col_map: &HashMap<&'static str, u32>,
    regime: Option<&RegimeData>,
) {
    let Some(regime) = regime else {
        return;
    };

    for (col_key, values) in [("opt_rus", &regime.opt_rvh), ("dop_rus", &regime.dop_rvh)] {
        let Some(col) = col_map.get(col_key).copied() else {
            continue;
        };
        let mut next = 0usize;
        for row in 1..=sheet.max_row() {
            let text = sheet.cell_text(row, col);
            if !RVH_RE.is_match(text) {
                continue;
            }
            let value = values.get(next).copied().flatten();
            next += 1;
            let Some(value) = value else {
                continue;
            };
            let formatted = format!("{value:.2}").replace('.', ",");
            let replaced = RVH_RE
                .replace(text, |caps: &regex::Captures<'_>| {
                    format!("{}{formatted}{}", &caps[1], &caps[2])
                })
                .into_owned();
            sheet.set_text(row, col, replaced);
        }
    }
}

/// Закэшированный результат формулы: <f>...</f><v>...</v> или <f .../><v>...</v>.
static FORMULA_CACHE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(</f>|<f[^>]*/>)<v[^>]*>[^<]*</v>").unwrap());

/// Карта «имя листа -> путь XML внутри архива» по workbook.xml и его rels.
fn workbook_sheet_paths<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<HashMap<String, String>> {
    let Some(workbook) = read_zip_entry(archive, "xl/workbook.xml")? else {
        return Ok(HashMap::new());
    };
    let Some(rels) = read_zip_entry(archive, "xl/_rels/workbook.xml.rels")? else {
        return Ok(HashMap::new());
    };

    let mut target_by_id = HashMap::new();
    for tag in RELATIONSHIP_TAG_RE.find_iter(&rels) {
        if let (Some(id), Some(target)) = (
            tag_attr(tag.as_str(), "Id"),
            tag_attr(tag.as_str(), "Target"),
        ) {
            let target = target.trim_start_matches('/');
            let target = if target.starts_with("xl/") {
                target.to_string()
            } else {
                format!("xl/{target}")
            };
            target_by_id.insert(id.to_string(), target);
        }
    }

    let mut paths = HashMap::new();
    for tag in SHEET_TAG_RE.find_iter(&workbook) {
        if let (Some(name), Some(rid)) = (
            tag_attr(tag.as_str(), "name"),
            tag_attr(tag.as_str(), "r:id"),
        ) && let Some(target) = target_by_id.get(rid)
        {
            paths.insert(name.to_string(), target.clone());
        }
    }
    Ok(paths)
}

fn tag_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    XML_ATTR_RE
        .captures_iter(tag)
        .find(|caps| &caps[1] == name)
        .and_then(|caps| caps.get(2))
        .map(|value| value.as_str())
}

fn read_zip_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<String>> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut value = String::new();
    entry.read_to_string(&mut value)?;
    Ok(Some(value))
}

static OUTPUT_SUFFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)_на_.+$").expect("valid output suffix regex"));
static TITLE_SEARCH_LC: Lazy<String> = Lazy::new(|| TITLE_SEARCH.to_lowercase());
static RVH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(\s*[pр]вх\s*=\s*)\S+(.*)$").expect("valid rvh regex"));
static RELATIONSHIP_TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<Relationship\b[^>]*>").expect("valid relationship tag regex"));
static SHEET_TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<sheet\b[^>]*>").expect("valid sheet tag regex"));
static XML_ATTR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"([\w:]+)="([^"]*)""#).expect("valid xml attr regex"));

fn next_month_period(year: i32, month: u32) -> Result<(i32, u32)> {
    if !(1..=12).contains(&month) {
        bail!("Месяц вне диапазона 1..12: {month}");
    }

    Ok(if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    })
}

fn target_period_label(year: i32, month: u32) -> Result<String> {
    let (target_year, target_month) = next_month_period(year, month)?;
    Ok(format!(
        "{} {} ГОДА",
        month_name_upper(target_month),
        target_year
    ))
}

fn target_period_title(year: i32, month: u32) -> Result<String> {
    Ok(format!(
        "{TITLE_SEARCH} {}",
        target_period_label(year, month)?
    ))
}

fn month_name_upper(month: u32) -> &'static str {
    match month {
        1 => "ЯНВАРЬ",
        2 => "ФЕВРАЛЬ",
        3 => "МАРТ",
        4 => "АПРЕЛЬ",
        5 => "МАЙ",
        6 => "ИЮНЬ",
        7 => "ИЮЛЬ",
        8 => "АВГУСТ",
        9 => "СЕНТЯБРЬ",
        10 => "ОКТЯБРЬ",
        11 => "НОЯБРЬ",
        12 => "ДЕКАБРЬ",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{RegimeParams, cap_regime_rus_to_rst, correct_work_pressure, regime_block};

    fn block(rus: f64, depr: f64, q: f64) -> Option<RegimeParams> {
        Some(RegimeParams {
            rus: Some(rus),
            depr: Some(depr),
            q: Some(q),
        })
    }

    fn assert_close(actual: Option<f64>, expected: f64, label: &str) {
        let actual = actual.unwrap_or_else(|| panic!("{label}: нет значения"));
        assert!(
            (actual - expected).abs() < 1e-9,
            "{label}: {actual} != {expected}"
        );
    }

    #[test]
    fn zero_debit_pins_rus_to_rst() {
        let (rus, depr) = correct_work_pressure(
            Some(0.0),
            Some(3.0),
            Some(1.0),
            Some(20.0),
            None,
            block(10.0, 3.0, 200.0),
        );
        assert_eq!((rus, depr), (Some(20.0), Some(0.0)));
    }

    #[test]
    fn missing_debit_keeps_raw_values() {
        let (rus, depr) = correct_work_pressure(
            None,
            Some(17.0),
            Some(0.7),
            Some(20.0),
            None,
            block(10.0, 3.0, 200.0),
        );
        assert_eq!((rus, depr), (Some(17.0), Some(0.7)));
    }

    #[test]
    fn line_goes_through_static_and_dop_point() {
        // депрессия — прямая через (0, 0) и (200, 3); Рус — через (0, 20)
        // и (200, 10); замеры из сводки не участвуют
        let (rus, depr) = correct_work_pressure(
            Some(150.0),
            Some(17.0),
            Some(0.1),
            Some(20.0),
            None,
            block(10.0, 3.0, 200.0),
        );
        assert_close(rus, 12.5, "Рус");
        assert_close(depr, 2.25, "депрессия");
    }

    #[test]
    fn missing_dop_keeps_raw_values() {
        let (rus, depr) =
            correct_work_pressure(Some(50.0), Some(17.0), Some(0.7), Some(20.0), None, None);
        assert_eq!((rus, depr), (Some(17.0), Some(0.7)));
        // точка с нулевыми давлениями непригодна для прямой
        let (rus, depr) = correct_work_pressure(
            Some(50.0),
            Some(17.0),
            Some(0.7),
            Some(20.0),
            None,
            block(0.0, 0.0, 200.0),
        );
        assert_eq!((rus, depr), (Some(17.0), Some(0.7)));
    }

    #[test]
    fn missing_rst_fits_depr_but_keeps_raw_rus() {
        let (rus, depr) = correct_work_pressure(
            Some(50.0),
            Some(17.0),
            Some(0.7),
            None,
            None,
            block(15.0, 1.5, 100.0),
        );
        assert_eq!(rus, Some(17.0));
        assert_close(depr, 0.75, "депрессия");
    }

    #[test]
    fn fitted_pressure_is_clamped_at_zero() {
        // далёкая экстраполяция уводит Рус в минус: прижимается к нулю
        let (rus, depr) = correct_work_pressure(
            Some(500.0),
            Some(1.0),
            Some(19.0),
            Some(20.0),
            None,
            block(15.0, 1.5, 100.0),
        );
        assert_close(rus, 0.0, "Рус");
        assert_close(depr, 7.5, "депрессия");
    }

    #[test]
    fn equal_rounded_debits_unify_opt_with_dop() {
        let (opt, dop) =
            super::unify_equal_regimes(block(19.34, 2.7, 45.0), block(19.337, 2.703, 45.0));
        assert_eq!(opt, block(19.337, 2.703, 45.0));
        assert_eq!(dop, block(19.337, 2.703, 45.0));
        // разные дебиты — режимы не трогаются
        let (opt, dop) =
            super::unify_equal_regimes(block(19.34, 2.7, 45.0), block(19.337, 2.703, 46.0));
        assert_eq!(opt, block(19.34, 2.7, 45.0));
        assert_eq!(dop, block(19.337, 2.703, 46.0));
    }

    #[test]
    fn work_debit_equal_to_regime_copies_its_params() {
        // дебит равен допустимому: параметры берутся с допустимого
        let (rus, depr) = correct_work_pressure(
            Some(200.0),
            Some(17.0),
            Some(0.1),
            Some(20.0),
            block(18.0, 1.0, 100.0),
            block(15.0, 2.0, 200.0),
        );
        assert_eq!((rus, depr), (Some(15.0), Some(2.0)));
        // дебит равен оптимальному: параметры берутся с оптимального
        let (rus, depr) = correct_work_pressure(
            Some(100.0),
            Some(17.0),
            Some(0.1),
            Some(20.0),
            block(18.0, 1.0, 100.0),
            block(15.0, 2.0, 200.0),
        );
        assert_eq!((rus, depr), (Some(18.0), Some(1.0)));
    }

    #[test]
    fn regime_block_values_are_rounded() {
        let rounded = RegimeParams {
            rus: Some(14.828892215686),
            depr: Some(0.09221775859407753),
            q: Some(14.4),
        }
        .rounded();
        assert_eq!(rounded, block(14.829, 0.092, 14.0).unwrap());
    }

    #[test]
    fn debit_above_dop_uses_static_and_dop() {
        let (rus, depr) = correct_work_pressure(
            Some(300.0),
            Some(17.0),
            Some(0.1),
            Some(20.0),
            block(18.0, 1.0, 100.0),
            block(15.0, 2.0, 200.0),
        );
        assert_close(rus, 12.5, "Рус");
        assert_close(depr, 3.0, "депрессия");
    }

    #[test]
    fn debit_below_opt_uses_static_and_opt() {
        // скв 10305/10306 НЫДА: дебит ниже оптимального — Рус обязан быть
        // выше Рус_опт, прямая идёт через статику и оптимальную точку
        let (rus, depr) = correct_work_pressure(
            Some(50.0),
            Some(17.0),
            Some(0.1),
            Some(20.0),
            block(18.0, 1.0, 100.0),
            block(15.0, 2.0, 200.0),
        );
        assert_close(rus, 19.0, "Рус");
        assert_close(depr, 0.5, "депрессия");
        assert!(rus.unwrap() > 18.0, "Рус должен быть выше Рус_опт");
    }

    #[test]
    fn debit_between_regimes_uses_both_points() {
        let (rus, depr) = correct_work_pressure(
            Some(150.0),
            Some(17.0),
            Some(0.1),
            Some(20.0),
            block(18.0, 1.0, 100.0),
            block(15.0, 2.0, 200.0),
        );
        assert_close(rus, 16.5, "Рус");
        assert_close(depr, 1.5, "депрессия");
        // между режимами прямая не требует Рст
        let (rus, _) = correct_work_pressure(
            Some(150.0),
            Some(17.0),
            Some(0.1),
            None,
            block(18.0, 1.0, 100.0),
            block(15.0, 2.0, 200.0),
        );
        assert_close(rus, 16.5, "Рус без Рст");
    }

    #[test]
    fn identical_regimes_use_static_and_dop() {
        let (rus, depr) = correct_work_pressure(
            Some(120.0),
            Some(17.0),
            Some(0.1),
            Some(20.0),
            block(15.0, 1.5, 100.0),
            block(15.0, 1.5, 100.0),
        );
        assert_close(rus, 14.0, "Рус");
        assert_close(depr, 1.8, "депрессия");
    }

    #[test]
    fn regime_rus_above_rst_is_replaced_on_all_regimes() {
        // Рус доп выше Рст: на обоих режимах Рус становится Рст - Депр
        let (opt, dop) =
            cap_regime_rus_to_rst(Some(13.5), block(13.0, 0.5, 100.0), block(14.0, 0.6, 120.0));
        assert_eq!(opt, block(13.0, 0.5, 100.0));
        assert_eq!(dop, block(12.9, 0.6, 120.0));
        // превышение только на оптимальном тоже чинит оба режима
        let (opt, dop) =
            cap_regime_rus_to_rst(Some(13.5), block(14.0, 0.5, 100.0), block(13.0, 0.6, 120.0));
        assert_eq!(opt, block(13.0, 0.5, 100.0));
        assert_eq!(dop, block(12.9, 0.6, 120.0));
    }

    #[test]
    fn consistent_regimes_stay_unchanged() {
        let (opt, dop) =
            cap_regime_rus_to_rst(Some(20.0), block(15.0, 1.5, 100.0), block(14.0, 2.0, 120.0));
        assert_eq!(opt, block(15.0, 1.5, 100.0));
        assert_eq!(dop, block(14.0, 2.0, 120.0));
    }

    #[test]
    fn capped_dop_feeds_the_fit() {
        // Рст 13.5 < Рус_доп 15.0: доп чинится до (13.0, 0.5), прямая Рус
        // идёт через (0, 13.5) и (100, 13.0)
        let (_, dop) = cap_regime_rus_to_rst(Some(13.5), None, block(15.0, 0.5, 100.0));
        assert_eq!(dop, block(13.0, 0.5, 100.0));
        let (rus, depr) =
            correct_work_pressure(Some(50.0), Some(14.0), Some(0.9), Some(13.5), None, dop);
        assert_close(rus, 13.25, "Рус");
        assert_close(depr, 0.25, "депрессия");
    }

    #[test]
    fn regime_block_with_zero_debit_pins_rus_to_rst() {
        let params = regime_block(Some(0.0), Some(12.3), Some(0.4), Some(20.0), 0.1);
        assert_eq!(Some(params), block(20.0, 0.4, 0.0));
        // без Рст значение из листа остаётся
        let params = regime_block(Some(0.0), Some(12.3), Some(0.4), None, 0.1);
        assert_eq!(params.rus, Some(12.3));
        // остановленная скважина не участвует в подстановке Рст - Депр
        let stopped = regime_block(Some(0.0), Some(12.3), Some(0.4), Some(12.0), 0.1);
        let (opt, _) = cap_regime_rus_to_rst(Some(12.0), Some(stopped), block(13.0, 0.6, 120.0));
        assert_eq!(opt, block(12.0, 0.4, 0.0));
    }

    #[test]
    fn stopped_regime_rus_above_rst_does_not_trigger_cap() {
        // Рус остановленного режима округлился чуть выше Рст: это не
        // противоречие данных, работающий режим не подставляется
        let stopped = block(14.33, 0.4, 0.0);
        let (opt, dop) = cap_regime_rus_to_rst(Some(14.3296), stopped, block(14.058, 0.092, 14.0));
        assert_eq!(opt, stopped);
        assert_eq!(dop, block(14.058, 0.092, 14.0));
    }
}
