//! Сборка прогнозных таблиц из выгрузок тНавигатора.
//!
//! Модуль повторяет расчёт формы «Прогноз тНавигатор»: агрегаты по пластам и
//! площадям строятся на шагах, попавших в выбор дат, а поскважинные сводные
//! таблицы — на окне `[первая выбранная дата; последняя выбранная дата)`.

use ahash::AHashMap as HashMap;
use anyhow::Result;
use chrono::NaiveDate;
use rayon::prelude::*;

use super::source::{LinkTable, OneCsv, ThreeCsv, ThreeRow, TwoCsv};
use super::{GdmField, Layout};

/// Колонки поскважинных листов — они же имена этих листов.
pub(super) const WELL_COLUMNS: [&str; 5] = ["wbp", "wthp", "wbhp", "wgpr", "wgpt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum DateStep {
    /// Каждое первое число каждого месяца.
    #[default]
    Month,
    /// Первое число месяца, следующего за кварталом (01.01, 01.04, 01.07, 01.10).
    Quarter,
    /// Каждое первое января.
    Year,
    /// Все шаги расчёта без прореживания.
    All,
}

impl DateStep {
    pub fn ui_label(self) -> &'static str {
        match self {
            Self::Month => "За месяц",
            Self::Quarter => "За квартал",
            Self::Year => "За год",
            Self::All => "Все даты",
        }
    }

    fn accepts(self, date: NaiveDate) -> bool {
        use chrono::Datelike;
        match self {
            Self::All => true,
            Self::Month => date.day() == 1,
            Self::Quarter => date.day() == 1 && matches!(date.month(), 1 | 4 | 7 | 10),
            Self::Year => date.day() == 1 && date.month() == 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub fields: Vec<GdmField>,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub step: DateStep,
}

/// Строка табличной части листа месторождения/площади/пласта.
pub(super) struct Row {
    pub(super) date: Box<str>,
    pub(super) values: Vec<Option<f64>>,
}

/// Лист с шапкой из шаблона и, возможно, дописанными блоками по пластам.
pub(super) struct SectionSheet {
    pub(super) title: String,
    pub(super) rows: Vec<Row>,
    pub(super) blocks: Vec<(String, Vec<Row>)>,
}

#[derive(Clone)]
pub(super) struct WellRow {
    pub(super) well_text: Box<str>,
    pub(super) well_number: Option<f64>,
    pub(super) gp: Box<str>,
    pub(super) plast: Box<str>,
    pub(super) plast_gp: Box<str>,
    pub(super) values: Vec<Option<f64>>,
}

#[derive(Clone)]
pub(super) struct ExtraRow {
    pub(super) label: Box<str>,
    pub(super) kind: &'static str,
    pub(super) values: Vec<Option<f64>>,
}

pub(super) struct DopSheet {
    pub(super) name: &'static str,
    pub(super) dates: Vec<Box<str>>,
    pub(super) wells: Vec<WellRow>,
    pub(super) extra: Vec<ExtraRow>,
}

pub(super) struct Book {
    pub(super) field: SectionSheet,
    pub(super) groups: Vec<SectionSheet>,
    pub(super) dop: Vec<DopSheet>,
}

/// Накопитель среднего/суммы, повторяющий пропуск NaN в pandas.
#[derive(Clone, Copy, Default)]
struct Acc {
    sum: f64,
    count: u32,
}

impl Acc {
    fn push(&mut self, value: f64) {
        if value.is_nan() {
            return;
        }
        self.sum += value;
        self.count += 1;
    }

    fn mean(self) -> Option<f64> {
        (self.count > 0).then(|| self.sum / f64::from(self.count))
    }

    fn total(self) -> Option<f64> {
        (self.count > 0).then_some(self.sum)
    }
}

/// Полный набор агрегатов одной категории (пласт / ГП / ГП+пласт) на одном шаге.
#[derive(Clone, Copy, Default)]
struct CatAcc {
    /// Строки с `wgpr > 0` — по ним считаются дебит и фонд скважин.
    wgpr: Acc,
    /// Накопленная добыча берётся по всем строкам категории.
    wgpt: Acc,
    wbp: Acc,
    wthp: Acc,
    wbhp: Acc,
}

/// Разобранные источники одного месторождения.
pub(super) struct FieldSources {
    pub(super) one: OneCsv,
    pub(super) two: TwoCsv,
    pub(super) three: ThreeCsv,
    pub(super) link: LinkTable,
}

/// Пласт и пласт_ГП каждой пары (ГП, скважина) после join со справочником.
struct EntityLinks {
    plast: Vec<Option<u32>>,
    plast_gp: Vec<Option<u32>>,
    plast_names: Vec<Box<str>>,
    plast_gp_names: Vec<Box<str>>,
}

pub(super) fn build(field: GdmField, sources: &FieldSources, request: &Request) -> Result<Book> {
    let three = &sources.three;
    let links = join_links(three, &sources.link);

    let selected = select_dates(&sources.one, request);
    // Индексы выбранных шагов внутри df_one — база для всех агрегатов.
    let mut id_to_row = vec![usize::MAX; three.ids.len()];
    for (row, one_index) in selected.iter().enumerate() {
        if let Some(id) = three.id_index.get(&sources.one.id[*one_index]) {
            id_to_row[*id as usize] = row;
        }
    }

    let groups = field.groups();
    let list_plast = sorted_names(&links.plast_names, &used_categories(three, &links.plast));

    let mest = mest_rows(&sources.one, &selected);
    let field_sheet = SectionSheet {
        title: field.sheet_title().to_string(),
        rows: mest,
        blocks: match field.layout() {
            Layout::OnePlast => Vec::new(),
            Layout::Hngkm | Layout::Bngkm => list_plast
                .par_iter()
                .map(|(plast_index, name)| {
                    (
                        name.to_string(),
                        plast_rows(three, &links, *plast_index, &id_to_row, selected.len()),
                    )
                })
                .collect(),
        },
    };

    let group_sheets = groups
        .par_iter()
        .map(|group| {
            let group_index = three
                .gps
                .iter()
                .position(|name| name.as_ref() == *group)
                .map(|index| index as u32);
            let rows = group_rows(
                &sources.two,
                &sources.one,
                three,
                group,
                group_index,
                &id_to_row,
                &selected,
            );
            let blocks = match field.layout() {
                Layout::Bngkm => list_plast
                    .iter()
                    .map(|(plast_index, name)| {
                        (
                            format!("{group}_{name}"),
                            group_plast_rows(
                                three,
                                &links,
                                group_index,
                                *plast_index,
                                &id_to_row,
                                selected.len(),
                            ),
                        )
                    })
                    .collect(),
                Layout::Hngkm | Layout::OnePlast => Vec::new(),
            };
            SectionSheet {
                title: (*group).to_string(),
                rows,
                blocks,
            }
        })
        .collect();

    let dop = build_dop_sheets(field, three, &links, &selected, &sources.one);

    Ok(Book {
        field: field_sheet,
        groups: group_sheets,
        dop,
    })
}

pub(super) fn select_dates_count(one: &OneCsv, request: &Request) -> usize {
    select_dates(one, request).len()
}

/// Отбирает шаги df_one по диапазону и шагу выгрузки.
fn select_dates(one: &OneCsv, request: &Request) -> Vec<usize> {
    one.date
        .iter()
        .enumerate()
        .filter(|(_, text)| {
            parse_date(text).is_some_and(|date| {
                date >= request.start && date <= request.end && request.step.accepts(date)
            })
        })
        .map(|(index, _)| index)
        .collect()
}

pub(super) fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%d.%m.%Y").ok()
}

fn join_links(three: &ThreeCsv, link: &LinkTable) -> EntityLinks {
    let mut plast_names: Vec<Box<str>> = Vec::new();
    let mut plast_index: HashMap<Box<str>, u32> = HashMap::new();
    let mut plast_gp_names: Vec<Box<str>> = Vec::new();
    let mut plast_gp_index: HashMap<Box<str>, u32> = HashMap::new();
    let mut plast = Vec::with_capacity(three.ent_gp.len());
    let mut plast_gp = Vec::with_capacity(three.ent_gp.len());

    for (entity, gp) in three.ent_gp.iter().enumerate() {
        let gp_name = &three.gps[*gp as usize];
        match link.get(gp_name, &three.ent_well[entity]) {
            Some((p, pg)) => {
                plast.push(Some(intern(&mut plast_names, &mut plast_index, p)));
                plast_gp.push(Some(intern(&mut plast_gp_names, &mut plast_gp_index, pg)));
            }
            None => {
                plast.push(None);
                plast_gp.push(None);
            }
        }
    }

    EntityLinks {
        plast,
        plast_gp,
        plast_names,
        plast_gp_names,
    }
}

fn intern(names: &mut Vec<Box<str>>, index: &mut HashMap<Box<str>, u32>, value: &str) -> u32 {
    if let Some(found) = index.get(value) {
        return *found;
    }
    let slot = names.len() as u32;
    names.push(Box::from(value));
    index.insert(Box::from(value), slot);
    slot
}

/// Категории, реально встречающиеся в поскважинной выгрузке.
fn used_categories(three: &ThreeCsv, of_entity: &[Option<u32>]) -> Vec<u32> {
    let mut seen = vec![
        false;
        of_entity
            .iter()
            .flatten()
            .copied()
            .max()
            .map_or(0, |m| m as usize + 1)
    ];
    for row in &three.rows {
        if let Some(category) = of_entity[row.ent as usize] {
            seen[category as usize] = true;
        }
    }
    seen.iter()
        .enumerate()
        .filter(|(_, used)| **used)
        .map(|(index, _)| index as u32)
        .collect()
}

fn sorted_names<'a>(names: &'a [Box<str>], used: &[u32]) -> Vec<(u32, &'a str)> {
    let mut out: Vec<(u32, &str)> = used
        .iter()
        .map(|index| (*index, names[*index as usize].as_ref()))
        .collect();
    out.sort_by(|left, right| left.1.cmp(right.1));
    out
}

fn mest_rows(one: &OneCsv, selected: &[usize]) -> Vec<Row> {
    let fgpt: Vec<f64> = selected.iter().map(|index| one.fgpt[*index]).collect();
    let diff = diff_series(&fgpt);
    selected
        .iter()
        .enumerate()
        .map(|(row, index)| Row {
            date: one.date[*index].clone(),
            values: vec![
                finite(one.fgpr[*index]),
                finite(diff[row]),
                finite(one.fgpt[*index]),
                finite(one.aaqt[*index]),
                finite(one.fpr[*index]),
                finite(one.fmwpr[*index]),
            ],
        })
        .collect()
}

/// Накапливает агрегаты по одной категории на каждом выбранном шаге.
fn accumulate<F>(three: &ThreeCsv, id_to_row: &[usize], rows: usize, matches: F) -> Vec<CatAcc>
where
    F: Fn(&ThreeRow) -> bool + Sync,
{
    let mut out = vec![CatAcc::default(); rows];
    for row in &three.rows {
        let slot = id_to_row[row.id as usize];
        if slot == usize::MAX || !matches(row) {
            continue;
        }
        let acc = &mut out[slot];
        acc.wgpt.push(row.wgpt);
        if row.wgpr > 0.0 {
            acc.wgpr.push(row.wgpr);
            if row.wbp > 0.0 {
                acc.wbp.push(row.wbp);
            }
            if row.wthp > 0.0 {
                acc.wthp.push(row.wthp);
            }
            if row.wbhp > 0.0 {
                acc.wbhp.push(row.wbhp);
            }
        }
    }
    out
}

/// Строки блока по пласту на листе месторождения.
///
/// Если на шаге нет ни одной работающей скважины пласта, соединение по ключу
/// `(id, date, plast)` в исходном скрипте рвётся и вся строка остаётся пустой —
/// повторяем это поведение.
fn plast_rows(
    three: &ThreeCsv,
    links: &EntityLinks,
    plast: u32,
    id_to_row: &[usize],
    rows: usize,
) -> Vec<Row> {
    let acc = accumulate(three, id_to_row, rows, |row| {
        links.plast[row.ent as usize] == Some(plast)
    });
    finish_plast_rows(three, id_to_row, &acc, rows, true)
}

/// Строки блока «ГП + пласт» на листе площади: ключ соединения задан явно,
/// поэтому пустой фонд скважин не обнуляет накопленную добычу.
fn group_plast_rows(
    three: &ThreeCsv,
    links: &EntityLinks,
    group: Option<u32>,
    plast: u32,
    id_to_row: &[usize],
    rows: usize,
) -> Vec<Row> {
    let acc = accumulate(three, id_to_row, rows, |row| {
        Some(three.ent_gp[row.ent as usize]) == group
            && links.plast[row.ent as usize] == Some(plast)
    });
    finish_plast_rows(three, id_to_row, &acc, rows, false)
}

fn finish_plast_rows(
    three: &ThreeCsv,
    id_to_row: &[usize],
    acc: &[CatAcc],
    rows: usize,
    gate_on_active_wells: bool,
) -> Vec<Row> {
    let dates = row_dates(three, id_to_row, rows);
    let wgpt: Vec<f64> = acc
        .iter()
        .map(|cat| {
            if gate_on_active_wells && cat.wgpr.count == 0 {
                f64::NAN
            } else {
                cat.wgpt.total().unwrap_or(f64::NAN)
            }
        })
        .collect();
    let diff = diff_series(&wgpt);

    acc.iter()
        .enumerate()
        .map(|(row, cat)| {
            let active = !gate_on_active_wells || cat.wgpr.count > 0;
            let wbp = active.then(|| cat.wbp.mean()).flatten();
            let wbhp = active.then(|| cat.wbhp.mean()).flatten();
            Row {
                date: dates[row].clone(),
                values: vec![
                    active.then(|| cat.wgpr.total()).flatten(),
                    finite(diff[row]),
                    finite(wgpt[row]),
                    wbp,
                    active.then(|| cat.wthp.mean()).flatten(),
                    match (wbp, wbhp) {
                        (Some(bottom), Some(hole)) => Some(bottom - hole),
                        _ => None,
                    },
                    active
                        .then(|| cat.wgpr.mean())
                        .flatten()
                        .map(|mean| mean * 1000.0),
                    (active && cat.wgpr.count > 0).then(|| f64::from(cat.wgpr.count)),
                ],
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn group_rows(
    two: &TwoCsv,
    one: &OneCsv,
    three: &ThreeCsv,
    group: &str,
    group_index: Option<u32>,
    id_to_row: &[usize],
    selected: &[usize],
) -> Vec<Row> {
    let acc = accumulate(three, id_to_row, selected.len(), |row| {
        Some(three.ent_gp[row.ent as usize]) == group_index
    });

    let selected_dates: HashMap<&str, usize> = selected
        .iter()
        .enumerate()
        .map(|(row, index)| (one.id[*index].as_ref(), row))
        .collect();

    let picked: Vec<usize> = (0..two.date.len())
        .filter(|index| {
            two.group[*index].as_ref() == group
                && selected_dates.contains_key(two.id[*index].as_ref())
        })
        .collect();

    let ggpt: Vec<f64> = picked.iter().map(|index| two.ggpt[*index]).collect();
    let diff = diff_series(&ggpt);

    picked
        .iter()
        .enumerate()
        .map(|(position, index)| {
            let slot = selected_dates.get(two.id[*index].as_ref()).copied();
            let cat = slot.map(|row| acc[row]).unwrap_or_default();
            let wbp = cat.wbp.mean();
            let wbhp = cat.wbhp.mean();
            Row {
                date: two.date[*index].clone(),
                values: vec![
                    finite(two.ggpr[*index]),
                    finite(diff[position]),
                    finite(two.ggpt[*index]),
                    wbp,
                    cat.wthp.mean(),
                    match (wbp, wbhp) {
                        (Some(bottom), Some(hole)) => Some(bottom - hole),
                        _ => None,
                    },
                    cat.wgpr.mean().map(|mean| mean * 1000.0),
                    finite(two.gmwpr[*index]),
                    finite(two.gnnpr[*index]),
                ],
            }
        })
        .collect()
}

fn row_dates(three: &ThreeCsv, id_to_row: &[usize], rows: usize) -> Vec<Box<str>> {
    let mut out = vec![Box::<str>::from(""); rows];
    for (id, row) in id_to_row.iter().enumerate() {
        if *row != usize::MAX {
            out[*row] = three.id_date[id].clone();
        }
    }
    out
}

/// `Series.diff()`: первый элемент пустой, дальше — разность с предыдущим.
fn diff_series(values: &[f64]) -> Vec<f64> {
    let mut out = vec![f64::NAN; values.len()];
    for index in 1..values.len() {
        out[index] = values[index] - values[index - 1];
    }
    out
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

// ---------------------------------------------------------------------------
// Поскважинные листы
// ---------------------------------------------------------------------------

/// Окно поскважинных данных: шаги `[первая выбранная дата; последняя)`.
struct Window<'a> {
    /// Индексы шагов окна в порядке сортировки идентификатора (как у pandas).
    ids: Vec<u32>,
    /// Для каждого шага — его позиция в колонках отчёта, либо `usize::MAX`.
    id_column: Vec<usize>,
    rows: Vec<&'a ThreeRow>,
    /// Множитель дебита на поскважинных листах: часть месторождений выводит
    /// его в тыс. м3/сут, БНГКМ — в исходных единицах.
    wgpr_scale: f64,
}

fn build_window<'a>(
    three: &'a ThreeCsv,
    one: &OneCsv,
    selected: &[usize],
    wgpr_scale: f64,
) -> Window<'a> {
    let (start, end) = match (selected.first(), selected.last()) {
        (Some(first), Some(last)) => (parse_date(&one.date[*first]), parse_date(&one.date[*last])),
        _ => (None, None),
    };
    let (Some(start), Some(end)) = (start, end) else {
        return Window {
            ids: Vec::new(),
            id_column: vec![usize::MAX; three.ids.len()],
            rows: Vec::new(),
            wgpr_scale,
        };
    };

    let in_window: Vec<bool> = three
        .id_date
        .iter()
        .map(|text| parse_date(text).is_some_and(|date| date >= start && date < end))
        .collect();

    let mut ids: Vec<u32> = (0..three.ids.len() as u32)
        .filter(|id| in_window[*id as usize])
        .collect();
    ids.sort_by(|left, right| three.ids[*left as usize].cmp(&three.ids[*right as usize]));

    let mut id_column = vec![usize::MAX; three.ids.len()];
    for (column, id) in ids.iter().enumerate() {
        id_column[*id as usize] = column;
    }

    let rows = three
        .rows
        .iter()
        .filter(|row| in_window[row.id as usize])
        .collect();

    Window {
        ids,
        id_column,
        rows,
        wgpr_scale,
    }
}

fn build_dop_sheets(
    field: GdmField,
    three: &ThreeCsv,
    links: &EntityLinks,
    selected: &[usize],
    one: &OneCsv,
) -> Vec<DopSheet> {
    let window = build_window(three, one, selected, field.well_wgpr_scale());
    let dates: Vec<Box<str>> = window
        .ids
        .iter()
        .map(|id| {
            let id = three.ids[*id as usize].as_ref();
            Box::<str>::from(&id[id.len().saturating_sub(10)..])
        })
        .collect();

    let wells = well_rows(three, links, &window);
    let extras = extra_rows(field, three, links, &window);

    field
        .dop_sheets()
        .iter()
        .enumerate()
        .map(|(index, name)| DopSheet {
            name,
            dates: dates.clone(),
            wells: wells[index].clone(),
            extra: extras[index].clone(),
        })
        .collect()
}

/// Сводная таблица «скважина × шаг» для каждой из пяти колонок.
fn well_rows(three: &ThreeCsv, links: &EntityLinks, window: &Window<'_>) -> Vec<Vec<WellRow>> {
    // Скважины без записи в справочнике выпадают из сводной таблицы: в pandas
    // groupby отбрасывает строки с NaN в индексе.
    let mut entities: Vec<u32> = {
        let mut seen = vec![false; three.ent_gp.len()];
        for row in &window.rows {
            seen[row.ent as usize] = true;
        }
        seen.iter()
            .enumerate()
            .filter(|(entity, used)| **used && links.plast[*entity].is_some())
            .map(|(entity, _)| entity as u32)
            .collect()
    };
    entities.sort_by(|left, right| {
        entity_key(three, links, *left).cmp(&entity_key(three, links, *right))
    });

    let mut entity_row = vec![usize::MAX; three.ent_gp.len()];
    for (row, entity) in entities.iter().enumerate() {
        entity_row[*entity as usize] = row;
    }

    let columns = window.ids.len();
    let cells = entities.len() * columns;
    let mut matrices: Vec<Vec<f64>> = (0..WELL_COLUMNS.len())
        .map(|_| vec![f64::NAN; cells])
        .collect();

    for row in &window.rows {
        let entity = entity_row[row.ent as usize];
        let column = window.id_column[row.id as usize];
        if entity == usize::MAX || column == usize::MAX {
            continue;
        }
        let offset = entity * columns + column;
        for (index, value) in well_values(row, window.wgpr_scale).into_iter().enumerate() {
            if matrices[index][offset].is_nan() && !value.is_nan() {
                matrices[index][offset] = value;
            }
        }
    }

    matrices
        .into_par_iter()
        .map(|matrix| {
            entities
                .iter()
                .enumerate()
                .map(|(row, entity)| {
                    let entity = *entity as usize;
                    let well = three.ent_well[entity].clone();
                    WellRow {
                        well_number: well_as_number(&well),
                        well_text: well,
                        gp: three.gps[three.ent_gp[entity] as usize].clone(),
                        plast: category_name(&links.plast_names, links.plast[entity]),
                        plast_gp: category_name(&links.plast_gp_names, links.plast_gp[entity]),
                        values: matrix[row * columns..(row + 1) * columns]
                            .iter()
                            .map(|value| finite(*value))
                            .collect(),
                    }
                })
                .collect()
        })
        .collect()
}

/// Работающая скважина: остальные не участвуют в средних и в фонде.
fn is_active(row: &ThreeRow) -> bool {
    row.wgpr > 0.0
}

fn well_values(row: &ThreeRow, wgpr_scale: f64) -> [f64; 5] {
    [row.wbp, row.wthp, row.wbhp, row.wgpr * wgpr_scale, row.wgpt]
}

/// Целиком числовой номер попадает в отчёт числом, боковой ствол — текстом.
fn well_as_number(well: &str) -> Option<f64> {
    well.parse::<i64>().ok().map(|number| number as f64)
}

/// Естественный порядок номеров: сначала числовой префикс, потом остаток, —
/// боковой ствол `1101_ZBS` встаёт сразу за материнской скважиной `1101`, а не
/// в конец списка, как при обычном сравнении строк.
fn well_sort_key(well: &str) -> (Option<i64>, Box<str>) {
    let digits = well.len() - well.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let (head, tail) = well.split_at(digits);
    (head.parse::<i64>().ok(), Box::from(tail))
}

/// Ключ сортировки строк поскважинного листа: номер скважины в естественном
/// порядке, дальше ГП, пласт и пласт_ГП.
type EntityKey = (Option<i64>, Box<str>, Box<str>, Box<str>, Box<str>);

fn entity_key(three: &ThreeCsv, links: &EntityLinks, entity: u32) -> EntityKey {
    let index = entity as usize;
    let (number, suffix) = well_sort_key(&three.ent_well[index]);
    (
        number,
        suffix,
        three.gps[three.ent_gp[index] as usize].clone(),
        category_name(&links.plast_names, links.plast[index]),
        category_name(&links.plast_gp_names, links.plast_gp[index]),
    )
}

fn category_name(names: &[Box<str>], index: Option<u32>) -> Box<str> {
    index.map_or_else(|| Box::from(""), |index| names[index as usize].clone())
}

/// Агрегатная функция строки-итога под таблицей скважин.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Agg {
    Mean,
    Sum,
    Count,
}

impl Agg {
    fn label(self) -> &'static str {
        match self {
            Self::Mean => "среднее",
            Self::Sum => "сумма",
            Self::Count => "count",
        }
    }
}

/// Ключ группировки строк-итогов.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ind {
    Gp,
    Plast,
    PlastGp,
}

fn extra_rows(
    field: GdmField,
    three: &ThreeCsv,
    links: &EntityLinks,
    window: &Window<'_>,
) -> Vec<Vec<ExtraRow>> {
    let inds = field.dop_inds();
    let mut out: Vec<Vec<ExtraRow>> = vec![Vec::new(); WELL_COLUMNS.len()];
    // В исходном скрипте для БНГКМ блок `среднее` живёт в переменной, которая
    // не обнуляется между итерациями: на колонке `wgpt` в отчёт попадает блок
    // с прошлого шага цикла. Воспроизводим это буквально.
    let mut last_block: Vec<ExtraRow> = Vec::new();

    for (column, name) in WELL_COLUMNS.iter().enumerate() {
        let mut result: Vec<ExtraRow> = Vec::new();
        for ind in inds {
            match field.layout() {
                Layout::OnePlast => {
                    if *ind == Ind::Gp {
                        result.extend(pivot_extra(
                            three,
                            links,
                            window,
                            *ind,
                            column,
                            Agg::Mean,
                            true,
                            false,
                        ));
                    }
                }
                Layout::Hngkm => {
                    if *name != "wgpt" {
                        result.extend(pivot_extra(
                            three,
                            links,
                            window,
                            *ind,
                            column,
                            Agg::Mean,
                            true,
                            false,
                        ));
                    } else {
                        result.extend(pivot_extra(
                            three,
                            links,
                            window,
                            *ind,
                            column,
                            Agg::Sum,
                            false,
                            false,
                        ));
                    }
                    if *name == "wgpr" {
                        result.extend(pivot_extra(
                            three,
                            links,
                            window,
                            *ind,
                            column,
                            Agg::Sum,
                            true,
                            false,
                        ));
                        result.extend(pivot_extra(
                            three,
                            links,
                            window,
                            *ind,
                            column,
                            Agg::Count,
                            true,
                            false,
                        ));
                    }
                }
                Layout::Bngkm => {
                    if *name != "wgpt" {
                        last_block =
                            pivot_extra(three, links, window, *ind, column, Agg::Mean, true, false);
                    }
                    result.extend(last_block.iter().cloned());
                    if *name == "wgpt" {
                        last_block =
                            pivot_extra(three, links, window, *ind, column, Agg::Sum, false, true);
                        result.extend(last_block.iter().cloned());
                    }
                    if *name == "wgpr" {
                        last_block =
                            pivot_extra(three, links, window, *ind, column, Agg::Sum, true, false);
                        result.extend(last_block.iter().cloned());
                        last_block = pivot_extra(
                            three,
                            links,
                            window,
                            *ind,
                            column,
                            Agg::Count,
                            true,
                            false,
                        );
                        result.extend(last_block.iter().cloned());
                    }
                }
            }
        }
        out[column] = result;
    }

    out
}

/// Одна сводная таблица итогов: строки — значения `ind`, колонки — шаги окна.
///
/// `only_active` ограничивает выборку работающими скважинами (`wgpr > 0`),
/// `detached_columns` повторяет ошибку исходного скрипта, где сводная строилась
/// по датам, а переиндексация шла по идентификаторам шага — колонки в итоге
/// оставались пустыми.
#[allow(clippy::too_many_arguments)]
fn pivot_extra(
    three: &ThreeCsv,
    links: &EntityLinks,
    window: &Window<'_>,
    ind: Ind,
    column: usize,
    agg: Agg,
    only_active: bool,
    detached_columns: bool,
) -> Vec<ExtraRow> {
    let (names, of_entity) = match ind {
        Ind::Gp => (&three.gps, None),
        Ind::Plast => (&links.plast_names, Some(&links.plast)),
        Ind::PlastGp => (&links.plast_gp_names, Some(&links.plast_gp)),
    };

    let key_of = |entity: u32| -> Option<u32> {
        match of_entity {
            Some(map) => map[entity as usize],
            None => Some(three.ent_gp[entity as usize]),
        }
    };

    let mut used = vec![false; names.len()];
    for row in &window.rows {
        if only_active && !is_active(row) {
            continue;
        }
        if let Some(key) = key_of(row.ent) {
            used[key as usize] = true;
        }
    }
    let mut keys: Vec<u32> = used
        .iter()
        .enumerate()
        .filter(|(_, flag)| **flag)
        .map(|(index, _)| index as u32)
        .collect();
    keys.sort_by(|left, right| names[*left as usize].cmp(&names[*right as usize]));

    let columns = window.ids.len();
    let mut key_row = vec![usize::MAX; names.len()];
    for (row, key) in keys.iter().enumerate() {
        key_row[*key as usize] = row;
    }
    let mut acc = vec![Acc::default(); keys.len() * columns];

    if !detached_columns {
        for row in &window.rows {
            if only_active && !is_active(row) {
                continue;
            }
            let Some(key) = key_of(row.ent) else {
                continue;
            };
            let slot = key_row[key as usize];
            let position = window.id_column[row.id as usize];
            if slot == usize::MAX || position == usize::MAX {
                continue;
            }
            acc[slot * columns + position].push(well_values(row, window.wgpr_scale)[column]);
        }
    }

    keys.iter()
        .enumerate()
        .map(|(row, key)| ExtraRow {
            label: names[*key as usize].clone(),
            kind: agg.label(),
            values: (0..columns)
                .map(|position| {
                    let cell = acc[row * columns + position];
                    match agg {
                        Agg::Mean => cell.mean(),
                        Agg::Sum => cell.total(),
                        Agg::Count => (cell.count > 0).then(|| f64::from(cell.count)),
                    }
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_series_leaves_first_empty() {
        let diff = diff_series(&[1.0, 3.0, 6.0]);
        assert!(diff[0].is_nan());
        assert_eq!(diff[1], 2.0);
        assert_eq!(diff[2], 3.0);
    }

    #[test]
    fn quarter_step_keeps_quarter_starts() {
        let keep = |year, month, day| {
            DateStep::Quarter.accepts(NaiveDate::from_ymd_opt(year, month, day).expect("дата"))
        };
        assert!(keep(2024, 1, 1));
        assert!(keep(2024, 4, 1));
        assert!(!keep(2024, 5, 1));
        assert!(!keep(2024, 4, 2));
    }

    #[test]
    fn sidetrack_sorts_next_to_its_parent_well() {
        let mut wells = ["1102", "1101_ZBS", "1101", "2P", "15"];
        wells.sort_by_key(|well| well_sort_key(well));
        assert_eq!(wells, ["2P", "15", "1101", "1101_ZBS", "1102"]);
    }

    #[test]
    fn sidetrack_stays_text_in_the_report() {
        assert_eq!(well_as_number("1101"), Some(1101.0));
        assert_eq!(well_as_number("1101_ZBS"), None);
    }

    #[test]
    fn acc_skips_nan() {
        let mut acc = Acc::default();
        acc.push(f64::NAN);
        assert_eq!(acc.mean(), None);
        acc.push(2.0);
        acc.push(4.0);
        assert_eq!(acc.mean(), Some(3.0));
        assert_eq!(acc.total(), Some(6.0));
    }
}
