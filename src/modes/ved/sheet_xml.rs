//! Прямое заполнение листа xlsx: XML листа читается в лёгкую модель
//! «(строка, колонка) -> текст», правки ячеек накапливаются и вклеиваются
//! в исходный XML точечно. Книга целиком не разбирается и не
//! пересериализуется, а условное форматирование из `<extLst>` шаблона
//! сохраняется само собой.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use ahash::AHashMap as HashMap;
use anyhow::{Context, Result, ensure};
use once_cell::sync::Lazy;
use regex::Regex;

pub(super) struct SheetXml {
    xml: String,
    cells: HashMap<(u32, u32), Cell>,
    rows: BTreeMap<u32, Row>,
    /// Начала объединённых диапазонов (строка, колонка).
    merge_starts: Vec<(u32, u32)>,
    max_row: u32,
    max_col: u32,
    edits: BTreeMap<(u32, u32), CellEdit>,
}

struct Cell {
    /// Байтовый диапазон всего элемента `<c ...>` в XML листа.
    range: Range<usize>,
    style: Option<String>,
    text: String,
    is_formula: bool,
}

struct Row {
    /// Диапазон всего элемента `<row ...>`.
    range: Range<usize>,
    /// Позиция перед `</row>` (для открытого элемента).
    content_end: usize,
    self_closing: bool,
}

#[derive(Clone, PartialEq)]
pub(super) enum CellValue {
    Number(f64),
    Text(String),
    Blank,
}

#[derive(Default)]
struct CellEdit {
    value: Option<CellValue>,
    yellow: bool,
}

impl SheetXml {
    pub(super) fn parse(xml: String, shared: &[String]) -> Result<Self> {
        let mut cells = HashMap::new();
        let mut rows = BTreeMap::new();
        let mut max_row = 0u32;
        let mut max_col = 0u32;

        let data_start = xml.find("<sheetData").unwrap_or(0);
        let data_end = xml.find("</sheetData>").unwrap_or(xml.len());
        let mut pos = data_start;
        while let Some(found) = find_element(&xml[..data_end], pos, "row") {
            let (tag_range, element_range, self_closing) = found;
            let tag = &xml[tag_range.clone()];
            let row_number: u32 = attr(tag, "r")
                .and_then(|value| value.parse().ok())
                .with_context(|| format!("Строка листа без номера: {tag}"))?;
            max_row = max_row.max(row_number);

            if !self_closing {
                let content_start = tag_range.end;
                let content_end = element_range.end - "</row>".len();
                let mut cell_pos = content_start;
                while let Some((cell_tag, cell_element, cell_closed)) =
                    find_element(&xml[..content_end], cell_pos, "c")
                {
                    let cell = parse_cell(&xml, &cell_tag, &cell_element, cell_closed, shared)?;
                    if let Some((coords, cell)) = cell {
                        max_col = max_col.max(coords.1);
                        cells.insert(coords, cell);
                    }
                    cell_pos = cell_element.end;
                }
                rows.insert(
                    row_number,
                    Row {
                        range: element_range.clone(),
                        content_end,
                        self_closing,
                    },
                );
            } else {
                rows.insert(
                    row_number,
                    Row {
                        range: element_range.clone(),
                        content_end: element_range.end,
                        self_closing,
                    },
                );
            }
            pos = element_range.end;
        }

        let mut merge_starts = Vec::new();
        let mut merge_pos = 0;
        while let Some((tag_range, element_range, _)) = find_element(&xml, merge_pos, "mergeCell") {
            if let Some(reference) = attr(&xml[tag_range.clone()], "ref")
                && let Some(start) = reference.split(':').next()
                && let Some(coords) = parse_cell_ref(start)
            {
                merge_starts.push(coords);
            }
            merge_pos = element_range.end;
        }

        Ok(Self {
            xml,
            cells,
            rows,
            merge_starts,
            max_row,
            max_col,
            edits: BTreeMap::new(),
        })
    }

    pub(super) fn cell_text(&self, row: u32, col: u32) -> &str {
        self.cells
            .get(&(row, col))
            .map(|cell| cell.text.as_str())
            .unwrap_or_default()
    }

    pub(super) fn max_row(&self) -> u32 {
        self.max_row
    }

    pub(super) fn max_col(&self) -> u32 {
        self.max_col
    }

    pub(super) fn merge_starts(&self) -> &[(u32, u32)] {
        &self.merge_starts
    }

    fn is_formula(&self, row: u32, col: u32) -> bool {
        self.cells
            .get(&(row, col))
            .is_some_and(|cell| cell.is_formula)
    }

    fn edit(&mut self, row: u32, col: u32) -> &mut CellEdit {
        self.edits.entry((row, col)).or_default()
    }

    /// Ячейки с формулами шаблона не перезаписываются и не перекрашиваются.
    pub(super) fn set_num_if_allowed(&mut self, row: u32, col: Option<u32>, value: Option<f64>) {
        let Some(col) = col else {
            return;
        };
        if self.is_formula(row, col) {
            return;
        }
        self.edit(row, col).value = Some(match value {
            Some(value) => CellValue::Number(value),
            None => CellValue::Blank,
        });
    }

    pub(super) fn set_date_if_allowed(&mut self, row: u32, col: Option<u32>, value: Option<i64>) {
        let Some(col) = col else {
            return;
        };
        if self.is_formula(row, col) {
            return;
        }
        self.edit(row, col).value = Some(match value {
            Some(value) => CellValue::Text(format!("{value:08}")),
            None => CellValue::Blank,
        });
    }

    pub(super) fn set_text(&mut self, row: u32, col: u32, text: String) {
        self.edit(row, col).value = Some(CellValue::Text(text));
    }

    /// Жёлтая заливка для ячеек, изменённых подстановкой Рст - Депр.
    pub(super) fn mark_cell_yellow(&mut self, row: u32, col: Option<u32>) {
        let Some(col) = col else {
            return;
        };
        if self.is_formula(row, col) {
            return;
        }
        self.edit(row, col).yellow = true;
    }

    /// Исходные номера стилей ячеек, помеченных жёлтым (для патча styles.xml).
    pub(super) fn yellow_source_styles(&self) -> BTreeSet<u32> {
        self.edits
            .iter()
            .filter(|(_, edit)| edit.yellow)
            .map(|(&coords, _)| self.style_id(coords))
            .collect()
    }

    fn style_id(&self, coords: (u32, u32)) -> u32 {
        self.cells
            .get(&coords)
            .and_then(|cell| cell.style.as_deref())
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    }

    /// XML листа с вклеенными правками; `yellow_styles` — карта
    /// «исходный стиль -> стиль с жёлтой заливкой».
    pub(super) fn render(&self, yellow_styles: &HashMap<u32, u32>) -> Result<String> {
        // (позиция, конец заменяемого диапазона, текст)
        let mut splices: Vec<(usize, usize, String)> = Vec::new();

        for (&(row, col), edit) in &self.edits {
            let style = match self.cells.get(&(row, col)) {
                Some(cell) => cell.style.clone(),
                None => None,
            };
            let style = if edit.yellow {
                let source = self.style_id((row, col));
                Some(
                    yellow_styles
                        .get(&source)
                        .with_context(|| format!("Нет жёлтого стиля для {source}"))?
                        .to_string(),
                )
            } else {
                style
            };

            let reference = format!("{}{row}", col_letters(col));
            let rendered = match &edit.value {
                Some(value) => render_cell(&reference, style.as_deref(), value),
                // Только заливка: содержимое ячейки сохраняется как есть.
                None => {
                    let cell = self
                        .cells
                        .get(&(row, col))
                        .context("Жёлтая заливка несуществующей ячейки")?;
                    let element = &self.xml[cell.range.clone()];
                    replace_style_attr(element, style.as_deref().unwrap_or("0"))
                }
            };

            match self.cells.get(&(row, col)) {
                Some(cell) => splices.push((cell.range.start, cell.range.end, rendered)),
                None => {
                    let Some(row_entry) = self.rows.get(&row) else {
                        // Строки нет в шаблоне: значение некуда писать.
                        continue;
                    };
                    if row_entry.self_closing {
                        // <row .../> разворачивается в открытый элемент
                        let tag = &self.xml[row_entry.range.clone()];
                        let opened =
                            format!("{}>{rendered}</row>", tag.trim_end_matches("/>").trim_end());
                        splices.push((row_entry.range.start, row_entry.range.end, opened));
                    } else {
                        let pos = self.insert_position(row, col, row_entry);
                        splices.push((pos, pos, rendered));
                    }
                }
            }
        }

        splices.sort_by_key(|&(start, end, _)| (start, end));
        let mut result = String::with_capacity(self.xml.len() + 1024);
        let mut cursor = 0usize;
        for (start, end, text) in splices {
            ensure!(start >= cursor, "Пересекающиеся правки XML листа");
            result.push_str(&self.xml[cursor..start]);
            result.push_str(&text);
            cursor = end;
        }
        result.push_str(&self.xml[cursor..]);
        Ok(result)
    }

    /// Позиция вставки новой ячейки: перед первой существующей ячейкой
    /// строки с большей колонкой, иначе перед `</row>`.
    fn insert_position(&self, row: u32, col: u32, row_entry: &Row) -> usize {
        self.cells
            .iter()
            .filter(|&(&(cell_row, cell_col), _)| cell_row == row && cell_col > col)
            .min_by_key(|&(&(_, cell_col), _)| cell_col)
            .map(|(_, cell)| cell.range.start)
            .unwrap_or(row_entry.content_end)
    }
}

fn parse_cell(
    xml: &str,
    tag_range: &Range<usize>,
    element_range: &Range<usize>,
    self_closing: bool,
    shared: &[String],
) -> Result<Option<((u32, u32), Cell)>> {
    let tag = &xml[tag_range.clone()];
    let Some(coords) = attr(tag, "r").and_then(parse_cell_ref) else {
        return Ok(None);
    };
    let style = attr(tag, "s").map(str::to_string);
    let cell_type = attr(tag, "t").unwrap_or("");
    let content = if self_closing {
        ""
    } else {
        &xml[tag_range.end..element_range.end - "</c>".len()]
    };
    let is_formula = content.contains("<f");

    let text = match cell_type {
        "s" => tag_text(content, "v")
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| shared.get(index).cloned())
            .unwrap_or_default(),
        "inlineStr" => concat_t_texts(content),
        _ => tag_text(content, "v").map(unescape_xml).unwrap_or_default(),
    };

    Ok(Some((
        (coords.0, coords.1),
        Cell {
            range: element_range.clone(),
            style,
            text: text.trim().to_string(),
            is_formula,
        },
    )))
}

fn render_cell(reference: &str, style: Option<&str>, value: &CellValue) -> String {
    let style_attr = style
        .map(|style| format!(" s=\"{style}\""))
        .unwrap_or_default();
    match value {
        CellValue::Number(number) => {
            format!("<c r=\"{reference}\"{style_attr}><v>{number}</v></c>")
        }
        CellValue::Text(text) => {
            let space = if text.trim().len() != text.len() {
                " xml:space=\"preserve\""
            } else {
                ""
            };
            format!(
                "<c r=\"{reference}\"{style_attr} t=\"inlineStr\"><is><t{space}>{}</t></is></c>",
                escape_xml(text)
            )
        }
        CellValue::Blank => format!("<c r=\"{reference}\"{style_attr}/>"),
    }
}

/// Замена (или добавление) атрибута s в существующем элементе ячейки.
fn replace_style_attr(element: &str, style: &str) -> String {
    static STYLE_ATTR_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#" s="[^"]*""#).expect("valid style attr regex"));
    if STYLE_ATTR_RE.is_match(element) {
        STYLE_ATTR_RE
            .replace(element, format!(" s=\"{style}\"").as_str())
            .into_owned()
    } else {
        element.replacen("<c ", &format!("<c s=\"{style}\" "), 1)
    }
}

/// Разбор sharedStrings.xml: текст каждого `<si>` — конкатенация его `<t>`.
pub(super) fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut pos = 0usize;
    while let Some((tag_range, element_range, self_closing)) = find_element(xml, pos, "si") {
        if self_closing {
            strings.push(String::new());
        } else {
            let content = &xml[tag_range.end..element_range.end - "</si>".len()];
            strings.push(concat_t_texts(content));
        }
        pos = element_range.end;
    }
    strings
}

fn concat_t_texts(content: &str) -> String {
    let mut text = String::new();
    let mut pos = 0usize;
    while let Some((tag_range, element_range, self_closing)) = find_element(content, pos, "t") {
        if !self_closing {
            let inner = &content[tag_range.end..element_range.end - "</t>".len()];
            text.push_str(&unescape_xml(inner));
        }
        pos = element_range.end;
    }
    text
}

/// Первый элемент `name` начиная с `from`: (диапазон открывающего тега,
/// диапазон всего элемента, самозакрывающийся ли). Вложенных элементов с тем
/// же именем в листах xlsx не бывает.
fn find_element(xml: &str, from: usize, name: &str) -> Option<(Range<usize>, Range<usize>, bool)> {
    let open = format!("<{name}");
    let mut search = from;
    loop {
        let start = xml[search..].find(&open)? + search;
        let after = xml.as_bytes().get(start + open.len())?;
        // не путать <c с <col, <row с <rowBreaks и т.п.
        if !matches!(after, b' ' | b'>' | b'/' | b'\t' | b'\r' | b'\n') {
            search = start + open.len();
            continue;
        }
        let tag_end = xml[start..].find('>')? + start + 1;
        if xml.as_bytes()[tag_end - 2] == b'/' {
            return Some((start..tag_end, start..tag_end, true));
        }
        let close = format!("</{name}>");
        let element_end = xml[tag_end..].find(&close)? + tag_end + close.len();
        return Some((start..tag_end, start..element_end, false));
    }
}

/// Значение атрибута открывающего тега. Ручной байтовый скан вместо regex:
/// вызывается для каждой ячейки листа, это горячий путь разбора.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut search = 0usize;
    while let Some(found) = tag[search..].find(name) {
        let start = search + found;
        let end = start + name.len();
        let preceded = start > 0 && matches!(bytes[start - 1], b' ' | b'\t' | b'\r' | b'\n');
        if preceded && tag[end..].starts_with("=\"") {
            let value_start = end + 2;
            let value_end = tag[value_start..].find('"')? + value_start;
            return Some(&tag[value_start..value_end]);
        }
        search = end;
    }
    None
}

fn tag_text<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let (tag_range, element_range, self_closing) = find_element(content, 0, name)?;
    if self_closing {
        return Some("");
    }
    Some(&content[tag_range.end..element_range.end - name.len() - 3])
}

/// «D25» -> (25, 4).
fn parse_cell_ref(reference: &str) -> Option<(u32, u32)> {
    let split = reference.find(|ch: char| ch.is_ascii_digit())?;
    let (letters, digits) = reference.split_at(split);
    let mut col = 0u32;
    for ch in letters.chars() {
        if !ch.is_ascii_uppercase() {
            return None;
        }
        col = col * 26 + (ch as u32 - 'A' as u32 + 1);
    }
    (col > 0).then_some(())?;
    Some((digits.parse().ok()?, col))
}

pub(super) fn col_letters(mut col: u32) -> String {
    let mut letters = Vec::new();
    while col > 0 {
        let rem = ((col - 1) % 26) as u8;
        letters.push(b'A' + rem);
        col = (col - 1) / 26;
    }
    letters.reverse();
    String::from_utf8(letters).expect("ascii letters")
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn unescape_xml(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Патч styles.xml: жёлтая заливка + производные стили для каждого исходного.
/// Возвращает новый XML и карту «исходный стиль -> жёлтый стиль».
pub(super) fn add_yellow_styles(
    styles_xml: &str,
    source_ids: &BTreeSet<u32>,
) -> Result<(String, HashMap<u32, u32>)> {
    static FILLS_COUNT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"<fills count="(\d+)""#).expect("valid fills regex"));
    static XFS_COUNT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"<cellXfs count="(\d+)""#).expect("valid cellXfs regex"));
    static FILL_ID_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"fillId="\d+""#).expect("valid fillId regex"));
    static APPLY_FILL_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"applyFill="[^"]*""#).expect("valid applyFill regex"));

    let fills_count: u32 = FILLS_COUNT_RE
        .captures(styles_xml)
        .and_then(|caps| caps[1].parse().ok())
        .context("В styles.xml нет блока fills")?;
    let yellow_fill_id = fills_count;
    let mut xml = FILLS_COUNT_RE
        .replace(
            styles_xml,
            format!("<fills count=\"{}\"", fills_count + 1).as_str(),
        )
        .into_owned();
    let fills_end = xml.find("</fills>").context("Нет </fills>")?;
    xml.insert_str(
        fills_end,
        "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFFFFF00\"/>\
         <bgColor indexed=\"64\"/></patternFill></fill>",
    );

    let xfs_count: u32 = XFS_COUNT_RE
        .captures(&xml)
        .and_then(|caps| caps[1].parse().ok())
        .context("В styles.xml нет блока cellXfs")?;
    let xfs_start = xml.find("<cellXfs").context("Нет <cellXfs>")?;
    let xfs_end = xml.find("</cellXfs>").context("Нет </cellXfs>")?;

    // существующие элементы <xf> по порядку
    let mut xf_elements = Vec::new();
    let mut pos = xfs_start;
    while let Some((_, element_range, _)) = find_element(&xml[..xfs_end], pos, "xf") {
        xf_elements.push(xml[element_range.clone()].to_string());
        pos = element_range.end;
    }
    ensure!(
        xf_elements.len() as u32 == xfs_count,
        "Число <xf> не совпадает с count в styles.xml"
    );

    let mut map = HashMap::new();
    let mut appended = String::new();
    for (index, &source) in source_ids.iter().enumerate() {
        let base = xf_elements
            .get(source as usize)
            .with_context(|| format!("Нет стиля {source} в styles.xml"))?;
        let mut derived = if FILL_ID_RE.is_match(base) {
            FILL_ID_RE
                .replace(base, format!("fillId=\"{yellow_fill_id}\"").as_str())
                .into_owned()
        } else {
            base.replacen("<xf ", &format!("<xf fillId=\"{yellow_fill_id}\" "), 1)
        };
        derived = if APPLY_FILL_RE.is_match(&derived) {
            APPLY_FILL_RE
                .replace(&derived, "applyFill=\"1\"")
                .into_owned()
        } else {
            derived.replacen("<xf ", "<xf applyFill=\"1\" ", 1)
        };
        appended.push_str(&derived);
        map.insert(source, xfs_count + index as u32);
    }

    let xfs_end = xml.find("</cellXfs>").context("Нет </cellXfs>")?;
    xml.insert_str(xfs_end, &appended);
    xml = XFS_COUNT_RE
        .replace(
            &xml,
            format!("<cellXfs count=\"{}\"", xfs_count + source_ids.len() as u32).as_str(),
        )
        .into_owned();
    Ok((xml, map))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &str = r#"<?xml version="1.0"?><worksheet><sheetData>
<row r="1" spans="1:3"><c r="A1" t="s" s="2"><v>0</v></c><c r="C1" s="3"><v>7</v></c></row>
<row r="2"><c r="A2" s="4"><f>SUM(A1)</f><v>9</v></c><c r="B2" t="inlineStr"><is><t>прив</t><t>ет</t></is></c></row>
<row r="3"/>
</sheetData><mergeCells count="1"><mergeCell ref="A1:C1"/></mergeCells></worksheet>"#;

    fn model() -> SheetXml {
        SheetXml::parse(SHEET.to_string(), &["Заголовок &1".to_string()]).unwrap()
    }

    #[test]
    fn parses_cells_shared_inline_and_formula() {
        let sheet = model();
        assert_eq!(sheet.cell_text(1, 1), "Заголовок &1");
        assert_eq!(sheet.cell_text(1, 3), "7");
        assert_eq!(sheet.cell_text(2, 2), "привет");
        assert_eq!(sheet.cell_text(2, 1), "9");
        assert!(sheet.is_formula(2, 1));
        assert!(!sheet.is_formula(1, 3));
        assert_eq!(sheet.max_row(), 3);
        assert_eq!(sheet.merge_starts(), &[(1, 1)]);
    }

    #[test]
    fn render_replaces_inserts_and_respects_formulas() {
        let mut sheet = model();
        sheet.set_num_if_allowed(1, Some(3), Some(2.5));
        sheet.set_num_if_allowed(2, Some(1), Some(99.0)); // формула — не трогать
        sheet.set_num_if_allowed(1, Some(2), Some(1.0)); // вставка между A1 и C1
        sheet.set_date_if_allowed(2, Some(3), Some(20260415)); // вставка в конец
        sheet.set_num_if_allowed(3, Some(1), Some(5.0)); // самозакрытая строка
        sheet.set_text(1, 1, "Новый <заголовок>".to_string());
        let rendered = sheet.render(&HashMap::new()).unwrap();
        assert!(rendered.contains(r#"<c r="C1" s="3"><v>2.5</v></c>"#));
        assert!(rendered.contains("<f>SUM(A1)</f>"), "формула сохранена");
        assert!(!rendered.contains("99"), "формульная ячейка не переписана");
        assert!(rendered.contains(r#"<c r="B1"><v>1</v></c><c r="C1""#));
        assert!(rendered.contains(r#"<c r="C2" t="inlineStr"><is><t>20260415</t></is></c></row>"#));
        assert!(rendered.contains(r#"<row r="3"><c r="A3"><v>5</v></c></row>"#));
        assert!(rendered.contains("Новый &lt;заголовок&gt;"));
    }

    #[test]
    fn yellow_mark_maps_style() {
        let mut sheet = model();
        sheet.set_num_if_allowed(1, Some(3), Some(2.5));
        sheet.mark_cell_yellow(1, Some(3));
        assert_eq!(sheet.yellow_source_styles(), BTreeSet::from([3]));
        let map = HashMap::from_iter([(3u32, 41u32)]);
        let rendered = sheet.render(&map).unwrap();
        assert!(rendered.contains(r#"<c r="C1" s="41"><v>2.5</v></c>"#));
    }

    #[test]
    fn add_yellow_styles_appends_fill_and_xfs() {
        let styles = r#"<styleSheet><fills count="2"><fill/><fill/></fills>
<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
<xf numFmtId="0" fontId="1" fillId="1" borderId="2" applyFill="0"><alignment wrapText="1"/></xf></cellXfs></styleSheet>"#;
        let (xml, map) = add_yellow_styles(styles, &BTreeSet::from([1])).unwrap();
        assert!(xml.contains(r#"<fills count="3""#));
        assert!(xml.contains(r#"fgColor rgb="FFFFFF00""#));
        assert!(xml.contains(r#"<cellXfs count="3""#));
        assert!(xml.contains(r#"<xf numFmtId="0" fontId="1" fillId="2" borderId="2" applyFill="1"><alignment wrapText="1"/></xf></cellXfs>"#));
        assert_eq!(map.get(&1), Some(&2));
    }

    #[test]
    fn cell_ref_roundtrip() {
        assert_eq!(parse_cell_ref("D25"), Some((25, 4)));
        assert_eq!(parse_cell_ref("AB3"), Some((3, 28)));
        assert_eq!(col_letters(4), "D");
        assert_eq!(col_letters(28), "AB");
    }

    #[test]
    fn shared_strings_concat_t() {
        let xml = r#"<sst><si><t>раз</t></si><si><r><t xml:space="preserve">два </t></r><r><t>три</t></r></si><si/></sst>"#;
        assert_eq!(
            parse_shared_strings(xml),
            vec!["раз".to_string(), "два три".to_string(), String::new()]
        );
    }
}
