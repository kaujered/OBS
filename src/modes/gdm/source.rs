//! Чтение исходников ГДМ: три CSV-выгрузки тНавигатора и справочник скважин.
//!
//! Формат CSV фиксирован скриптом тНавигатора (разделитель `;`, заголовок в
//! первой строке), поэтому читаем их вручную: это на порядок быстрее общего
//! CSV-парсера и позволяет разбирать самый тяжёлый `df_three.csv` (до сотни
//! мегабайт) параллельно по кускам строк.

use std::path::Path;

use ahash::AHashMap as HashMap;
use anyhow::{Context, Result, anyhow, bail};
use calamine::Reader;
use rayon::prelude::*;

use crate::tabular::sheet::open_workbook;
use crate::tabular::xlsx::cell_to_display_string;

/// Строки `df_one.csv` — показатели месторождения на каждый временной шаг.
pub(super) struct OneCsv {
    pub(super) date: Vec<Box<str>>,
    pub(super) id: Vec<Box<str>>,
    pub(super) fgpr: Vec<f64>,
    pub(super) fgpt: Vec<f64>,
    pub(super) aaqt: Vec<f64>,
    pub(super) fpr: Vec<f64>,
    pub(super) fmwpr: Vec<f64>,
}

/// Строки `df_two.csv` — показатели по площадям (ГП).
pub(super) struct TwoCsv {
    pub(super) date: Vec<Box<str>>,
    pub(super) group: Vec<Box<str>>,
    pub(super) id: Vec<Box<str>>,
    pub(super) ggpr: Vec<f64>,
    pub(super) ggpt: Vec<f64>,
    pub(super) gmwpr: Vec<f64>,
    pub(super) gnnpr: Vec<f64>,
}

/// Одна строка `df_three.csv` после интернирования ключей.
#[derive(Clone, Copy)]
pub(super) struct ThreeRow {
    /// Индекс временного шага в [`ThreeCsv::ids`].
    pub(super) id: u32,
    /// Индекс пары (ГП, скважина) в [`ThreeCsv::ent_gp`] / [`ThreeCsv::ent_well`].
    pub(super) ent: u32,
    pub(super) wgpr: f64,
    pub(super) wgpt: f64,
    pub(super) wbp: f64,
    pub(super) wthp: f64,
    pub(super) wbhp: f64,
}

/// Поскважинная выгрузка: ключи вынесены в словари, строки — плоский массив.
pub(super) struct ThreeCsv {
    pub(super) ids: Vec<Box<str>>,
    pub(super) id_date: Vec<Box<str>>,
    pub(super) id_index: HashMap<Box<str>, u32>,
    pub(super) gps: Vec<Box<str>>,
    /// Для каждой пары (ГП, скважина) — индекс ГП.
    pub(super) ent_gp: Vec<u32>,
    /// Для каждой пары (ГП, скважина) — канонический номер скважины.
    pub(super) ent_well: Vec<Box<str>>,
    pub(super) rows: Vec<ThreeRow>,
}

/// Справочник `справочник.xlsx`: (ГП, скважина) → (пласт, пласт_ГП).
type LinkKey = (Box<str>, Box<str>);
type LinkValue = (Box<str>, Box<str>);

pub(super) struct LinkTable {
    map: HashMap<LinkKey, LinkValue>,
}

impl LinkTable {
    pub(super) fn get(&self, gp: &str, well: &str) -> Option<(&str, &str)> {
        // Ключ приходится собирать из владеющих строк: пары ищутся один раз на
        // скважину, а не на строку выгрузки, поэтому это не горячий путь.
        let key = (Box::from(gp), Box::from(well));
        self.map
            .get(&key)
            .map(|(plast, plast_gp)| (plast.as_ref(), plast_gp.as_ref()))
    }
}

pub(super) fn read_one(path: &Path) -> Result<OneCsv> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let header = split_header(&mut lines, path)?;
    let [i_date, i_fgpr, i_fgpt, i_aaqt, i_fpr, i_fmwpr, i_id] = column_indexes(
        &header,
        ["date", "fgpr", "fgpt", "aaqt", "fpr", "fmwpr", "id"],
        path,
    )?;

    let mut out = OneCsv {
        date: Vec::new(),
        id: Vec::new(),
        fgpr: Vec::new(),
        fgpt: Vec::new(),
        aaqt: Vec::new(),
        fpr: Vec::new(),
        fmwpr: Vec::new(),
    };
    let mut fields = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        split_into(line, &mut fields);
        out.date.push(Box::from(field(&fields, i_date)));
        out.id.push(Box::from(field(&fields, i_id)));
        out.fgpr.push(parse_f64(field(&fields, i_fgpr)));
        out.fgpt.push(parse_f64(field(&fields, i_fgpt)));
        out.aaqt.push(parse_f64(field(&fields, i_aaqt)));
        out.fpr.push(parse_f64(field(&fields, i_fpr)));
        out.fmwpr.push(parse_f64(field(&fields, i_fmwpr)));
    }
    Ok(out)
}

pub(super) fn read_two(path: &Path) -> Result<TwoCsv> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let header = split_header(&mut lines, path)?;
    let [i_date, i_group, i_ggpr, i_ggpt, i_gmwpr, i_gnnpr, i_id] = column_indexes(
        &header,
        ["date", "group", "ggpr", "ggpt", "gmwpr", "gnnpr", "id"],
        path,
    )?;

    let mut out = TwoCsv {
        date: Vec::new(),
        group: Vec::new(),
        id: Vec::new(),
        ggpr: Vec::new(),
        ggpt: Vec::new(),
        gmwpr: Vec::new(),
        gnnpr: Vec::new(),
    };
    let mut fields = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        split_into(line, &mut fields);
        out.date.push(Box::from(field(&fields, i_date)));
        out.group.push(Box::from(field(&fields, i_group)));
        out.id.push(Box::from(field(&fields, i_id)));
        out.ggpr.push(parse_f64(field(&fields, i_ggpr)));
        out.ggpt.push(parse_f64(field(&fields, i_ggpt)));
        out.gmwpr.push(parse_f64(field(&fields, i_gmwpr)));
        out.gnnpr.push(parse_f64(field(&fields, i_gnnpr)));
    }
    Ok(out)
}

/// Промежуточное представление строки `df_three.csv`: ключи ещё заимствованы
/// из буфера файла, числа уже разобраны.
struct RawThree<'a> {
    date: &'a str,
    gp: &'a str,
    well: &'a str,
    id: &'a str,
    wgpr: f64,
    wgpt: f64,
    wbp: f64,
    wthp: f64,
    wbhp: f64,
}

pub(super) fn read_three(path: &Path) -> Result<ThreeCsv> {
    let text = read_text(path)?;
    let mut lines = text.lines();
    let header = split_header(&mut lines, path)?;
    let [
        i_date,
        i_gp,
        i_well,
        i_wgpr,
        i_wgpt,
        i_wbp,
        i_wthp,
        i_wbhp,
        i_id,
    ] = column_indexes(
        &header,
        [
            "date", "gp", "well", "wgpr", "wgpt", "wbp", "wthp", "wbhp", "id",
        ],
        path,
    )?;

    let body: Vec<&str> = lines.filter(|line| !line.trim().is_empty()).collect();
    let raw: Vec<RawThree<'_>> = body
        .par_iter()
        .map(|line| {
            let mut fields = Vec::new();
            split_into(line, &mut fields);
            RawThree {
                date: field(&fields, i_date),
                gp: field(&fields, i_gp),
                well: field(&fields, i_well),
                id: field(&fields, i_id),
                wgpr: parse_f64(field(&fields, i_wgpr)),
                wgpt: parse_f64(field(&fields, i_wgpt)),
                wbp: parse_f64(field(&fields, i_wbp)),
                wthp: parse_f64(field(&fields, i_wthp)),
                wbhp: parse_f64(field(&fields, i_wbhp)),
            }
        })
        .collect();

    let mut ids: Vec<Box<str>> = Vec::new();
    let mut id_date: Vec<Box<str>> = Vec::new();
    let mut id_index: HashMap<Box<str>, u32> = HashMap::new();
    let mut gps: Vec<Box<str>> = Vec::new();
    let mut gp_index: HashMap<Box<str>, u32> = HashMap::new();
    let mut ent_gp: Vec<u32> = Vec::new();
    let mut ent_well: Vec<Box<str>> = Vec::new();
    let mut ent_index: HashMap<(u32, Box<str>), u32> = HashMap::new();
    let mut rows = Vec::with_capacity(raw.len());

    for row in &raw {
        let id = match id_index.get(row.id) {
            Some(index) => *index,
            None => {
                let index = ids.len() as u32;
                ids.push(Box::from(row.id));
                id_date.push(Box::from(row.date));
                id_index.insert(Box::from(row.id), index);
                index
            }
        };
        let gp = match gp_index.get(row.gp) {
            Some(index) => *index,
            None => {
                let index = gps.len() as u32;
                gps.push(Box::from(row.gp));
                gp_index.insert(Box::from(row.gp), index);
                index
            }
        };
        let well = canonical_well(row.well);
        let ent = match ent_index.get(&(gp, well.clone())) {
            Some(index) => *index,
            None => {
                let index = ent_gp.len() as u32;
                ent_gp.push(gp);
                ent_well.push(well.clone());
                ent_index.insert((gp, well), index);
                index
            }
        };
        rows.push(ThreeRow {
            id,
            ent,
            wgpr: row.wgpr,
            wgpt: row.wgpt,
            wbp: row.wbp,
            wthp: row.wthp,
            wbhp: row.wbhp,
        });
    }

    Ok(ThreeCsv {
        ids,
        id_date,
        id_index,
        gps,
        ent_gp,
        ent_well,
        rows,
    })
}

pub(super) fn read_link(path: &Path, sheet: &str) -> Result<LinkTable> {
    let mut workbook = open_workbook(path)?;
    let range = workbook
        .worksheet_range(sheet)
        .with_context(|| format!("Лист «{sheet}» не найден в {}", path.display()))?;

    let mut header: HashMap<String, usize> = HashMap::new();
    let mut iter = range.rows();
    let Some(first) = iter.next() else {
        bail!("Лист «{sheet}» справочника пуст.");
    };
    for (index, cell) in first.iter().enumerate() {
        header.insert(cell_to_display_string(cell).trim().to_lowercase(), index);
    }
    let index_of = |name: &str| -> Result<usize> {
        header
            .get(name)
            .copied()
            .ok_or_else(|| anyhow!("В листе «{sheet}» справочника нет колонки «{name}»."))
    };
    let (i_well, i_gp, i_plast, i_plast_gp) = (
        index_of("well")?,
        index_of("gp")?,
        index_of("plast")?,
        index_of("plast_gp")?,
    );

    let mut map = HashMap::new();
    for row in iter {
        let Some(gp) = row.get(i_gp).map(cell_to_display_string) else {
            continue;
        };
        let Some(well) = row.get(i_well).map(cell_to_display_string) else {
            continue;
        };
        let gp = gp.trim();
        let well = well.trim();
        if gp.is_empty() || well.is_empty() {
            continue;
        }
        let plast = row
            .get(i_plast)
            .map(cell_to_display_string)
            .unwrap_or_default();
        let plast_gp = row
            .get(i_plast_gp)
            .map(cell_to_display_string)
            .unwrap_or_default();
        if plast.trim().is_empty() {
            continue;
        }
        map.insert(
            (Box::from(gp), canonical_well(well)),
            (Box::from(plast.trim()), Box::from(plast_gp.trim())),
        );
    }

    Ok(LinkTable { map })
}

/// Номер скважины всегда строковый — иначе в справочнике нельзя задать боковой
/// ствол вроде `1101_ZBS`. Целиком числовые номера приводятся к целому, чтобы
/// `0941` из выгрузки нашёл запись `941`, записанную в Excel числом.
fn canonical_well(value: &str) -> Box<str> {
    let trimmed = value.trim();
    match trimmed.parse::<i64>() {
        Ok(number) => Box::from(number.to_string()),
        Err(_) => Box::from(trimmed),
    }
}

fn read_text(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Не удалось прочитать {}", path.display()))?;
    // Выгрузки тНавигатора — валидный UTF-8, и тогда буфер переиспользуется без
    // копии; на битых байтах откатываемся к замене на U+FFFD.
    Ok(String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()))
}

fn split_header<'a>(lines: &mut std::str::Lines<'a>, path: &Path) -> Result<Vec<&'a str>> {
    let Some(first) = lines.next() else {
        bail!("Файл {} пуст.", path.display());
    };
    Ok(first
        .trim_end_matches('\r')
        .split(';')
        .map(str::trim)
        .collect())
}

fn column_indexes<const N: usize>(
    header: &[&str],
    names: [&str; N],
    path: &Path,
) -> Result<[usize; N]> {
    let mut out = [0usize; N];
    for (slot, name) in out.iter_mut().zip(names) {
        *slot = header
            .iter()
            .position(|column| column.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow!("В {} нет колонки «{name}».", path.display()))?;
    }
    Ok(out)
}

fn split_into<'a>(line: &'a str, out: &mut Vec<&'a str>) {
    out.clear();
    out.extend(line.trim_end_matches('\r').split(';'));
}

fn field<'a>(fields: &[&'a str], index: usize) -> &'a str {
    fields.get(index).copied().unwrap_or("").trim()
}

fn parse_f64(value: &str) -> f64 {
    if value.is_empty() {
        return f64::NAN;
    }
    value.parse::<f64>().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_well_drops_leading_zeros_for_numbers() {
        assert_eq!(canonical_well(" 0941 ").as_ref(), "941");
    }

    #[test]
    fn canonical_well_keeps_sidetrack_names() {
        assert_eq!(canonical_well(" 1101_ZBS ").as_ref(), "1101_ZBS");
        assert_eq!(canonical_well("1P").as_ref(), "1P");
    }

    #[test]
    fn parse_f64_maps_empty_to_nan() {
        assert!(parse_f64("").is_nan());
        assert_eq!(parse_f64("1.5"), 1.5);
    }

    #[test]
    fn column_indexes_reports_missing_column() {
        let header = ["date", "fgpr"];
        let error = column_indexes(&header, ["date", "fgpt"], Path::new("x.csv"))
            .expect_err("колонки fgpt нет");
        assert!(error.to_string().contains("fgpt"));
    }
}
