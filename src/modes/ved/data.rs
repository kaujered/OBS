//! Data loading and normalization for `VED`.

use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use ahash::AHashMap as HashMap;

use super::*;
use crate::tabular::dbf::{DbfField, DbfRecordView, DbfTable};

#[derive(Debug, Clone, Default)]
pub(super) struct CombinedRow {
    pub(super) date_key: Option<i64>,
    pub(super) rst: Option<f64>,
    pub(super) rpl: Option<f64>,
    pub(super) q_work: Option<f64>,
    pub(super) depr: Option<f64>,
    pub(super) rus: Option<f64>,
    pub(super) rzt: Option<f64>,
    pub(super) rshl: Option<f64>,
    pub(super) tus: Option<f64>,
    pub(super) rmk: Option<f64>,
    pub(super) rvh: Option<f64>,
    pub(super) tvh: Option<f64>,
    pub(super) qwat: Option<f64>,
    pub(super) qdop: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SsRow {
    well: i64,
    mest_code: i32,
    date_raw: i64,
    values: SsLastRow,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PplRow {
    well: i64,
    mest_code: i32,
    date_raw: i64,
    rst: Option<f64>,
    rpl: Option<f64>,
}

/// Замеры суточной сводки по скважине (без ключей записи).
#[derive(Debug, Clone, Copy)]
pub(super) struct SsLastRow {
    pub(super) q_work: Option<f64>,
    pub(super) depr: Option<f64>,
    pub(super) rus: Option<f64>,
    pub(super) rzt: Option<f64>,
    pub(super) rshl: Option<f64>,
    pub(super) tus: Option<f64>,
    pub(super) rmk: Option<f64>,
    pub(super) rvh: Option<f64>,
    pub(super) tvh: Option<f64>,
    pub(super) qwat: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct PplLastRow {
    pub(super) date_meas: i64,
    pub(super) rst: Option<f64>,
    pub(super) rpl: Option<f64>,
}

pub(super) struct DataStore {
    ss_rows: Vec<SsRow>,
    ppl_rows: Vec<PplRow>,
    qdop_by_mest: HashMap<i32, HashMap<i64, f64>>,
    ss_is_daily: bool,
}

/// Ключ кэша данных: источники (mtime + размер), месторождения и период.
#[derive(PartialEq)]
struct StoreCacheKey {
    sources: Vec<Option<(SystemTime, u64)>>,
    codes: BTreeSet<i32>,
    report_int: i64,
    report_yyyymm: i64,
}

static STORE_CACHE: Mutex<Option<(StoreCacheKey, Arc<DataStore>)>> = Mutex::new(None);

impl DataStore {
    /// Кэш разобранных данных в памяти процесса: повторная выгрузка с теми
    /// же источниками, месторождениями и периодом не перечитывает DBF.
    /// Работает в GUI, где процесс живёт между нажатиями «Выгрузить»;
    /// изменение mtime или размера любого источника сбрасывает кэш.
    pub(super) fn load_cached(
        paths: &paths::VedPaths,
        selected_codes: &BTreeSet<i32>,
        report_int: i64,
        report_yyyymm: i64,
    ) -> Result<Arc<Self>> {
        let key = StoreCacheKey {
            sources: [&paths.ss_dbf, &paths.ppl_dbf, &paths.wells_xlsx]
                .into_iter()
                .map(|path| {
                    std::fs::metadata(path)
                        .ok()
                        .and_then(|meta| Some((meta.modified().ok()?, meta.len())))
                })
                .collect(),
            codes: selected_codes.clone(),
            report_int,
            report_yyyymm,
        };

        if let Some((cached_key, store)) = STORE_CACHE.lock().expect("store cache lock").as_ref()
            && *cached_key == key
        {
            return Ok(Arc::clone(store));
        }

        let store = Arc::new(Self::load(
            paths,
            selected_codes,
            report_int,
            report_yyyymm,
        )?);
        *STORE_CACHE.lock().expect("store cache lock") = Some((key, Arc::clone(&store)));
        Ok(store)
    }

    fn load(
        paths: &paths::VedPaths,
        selected_codes: &BTreeSet<i32>,
        report_int: i64,
        report_yyyymm: i64,
    ) -> Result<Self> {
        // Load independent sources in parallel: this is one of the most expensive
        // startup steps in the whole project.
        let ((ss_rows, ppl_rows), qdop) = rayon::join(
            || {
                rayon::join(
                    || load_ss_rows(&paths.ss_dbf, selected_codes, report_int, report_yyyymm),
                    || load_ppl_rows(&paths.ppl_dbf, selected_codes),
                )
            },
            || load_qdop_excel(&paths.wells_xlsx, selected_codes),
        );

        let ss_rows =
            ss_rows.with_context(|| format!("Не удалось прочитать {}", paths.ss_dbf.display()))?;
        let ppl_rows = ppl_rows
            .with_context(|| format!("Не удалось прочитать {}", paths.ppl_dbf.display()))?;
        let qdop =
            qdop.with_context(|| format!("Не удалось прочитать {}", paths.wells_xlsx.display()))?;
        let ss_is_daily = ss_rows.iter().any(|row| row.date_raw >= 10_000_000);

        Ok(Self {
            ss_rows,
            ppl_rows,
            qdop_by_mest: qdop,
            ss_is_daily,
        })
    }

    pub(super) fn ss_last_by_well(
        &self,
        mest_code: Option<i32>,
        report_int: i64,
        report_yyyymm: i64,
    ) -> HashMap<i64, SsLastRow> {
        let target_date = if self.ss_is_daily {
            report_int
        } else {
            report_yyyymm
        };

        let mut last = HashMap::with_capacity(self.ss_rows.len().min(512));
        for row in &self.ss_rows {
            if mest_code.is_some_and(|code| row.mest_code != code) || row.date_raw != target_date {
                continue;
            }
            last.insert(row.well, row.values);
        }

        last
    }

    pub(super) fn ppl_last_by_well(
        &self,
        mest_code: Option<i32>,
        report_int: i64,
    ) -> HashMap<i64, PplLastRow> {
        let mut last = HashMap::<i64, PplLastRow>::with_capacity(self.ppl_rows.len().min(512));

        for row in &self.ppl_rows {
            if mest_code.is_some_and(|code| row.mest_code != code)
                || row.date_raw > report_int
                || row.rpl.is_none()
            {
                continue;
            }

            let should_replace = match last.get(&row.well) {
                Some(prev) => row.date_raw >= prev.date_meas,
                None => true,
            };

            if should_replace {
                last.insert(
                    row.well,
                    PplLastRow {
                        date_meas: row.date_raw,
                        rst: row.rst,
                        rpl: row.rpl,
                    },
                );
            }
        }

        last
    }

    pub(super) fn qdop_for_mest(&self, mest_code: Option<i32>) -> Option<&HashMap<i64, f64>> {
        mest_code.and_then(|code| self.qdop_by_mest.get(&code))
    }
}

impl CombinedRow {
    #[inline]
    fn with_ss_defaults() -> Self {
        Self {
            date_key: None,
            rst: None,
            rpl: None,
            q_work: Some(0.0),
            depr: Some(0.0),
            rus: Some(0.0),
            rzt: Some(0.0),
            rshl: Some(0.0),
            tus: Some(0.0),
            rmk: Some(0.0),
            rvh: Some(0.0),
            tvh: Some(0.0),
            qwat: Some(0.0),
            qdop: None,
        }
    }
}

/// Поля суточной сводки в eer1.dbf.
struct SsFields {
    bb1: DbfField,
    bb3: DbfField,
    bb4: DbfField,
    bs1: DbfField,
    bs11: DbfField,
    bs12: DbfField,
    bs13: DbfField,
    bs15: DbfField,
    bs16: DbfField,
    bs17: DbfField,
    bs19: DbfField,
    bs21: DbfField,
    bs22: DbfField,
    bs23: DbfField,
}

impl SsFields {
    fn resolve(table: &DbfTable) -> Result<Self> {
        Ok(Self {
            bb1: table.field("BB1")?,
            bb3: table.field("BB3")?,
            bb4: table.field("BB4")?,
            bs1: table.field("BS1")?,
            bs11: table.field("BS11")?,
            bs12: table.field("BS12")?,
            bs13: table.field("BS13")?,
            bs15: table.field("BS15")?,
            bs16: table.field("BS16")?,
            bs17: table.field("BS17")?,
            bs19: table.field("BS19")?,
            bs21: table.field("BS21")?,
            bs22: table.field("BS22")?,
            bs23: table.field("BS23")?,
        })
    }
}

/// Сводка (149+ МБ) читается с хвоста: записи в eer1.dbf дописываются
/// хронологически, а ведомости нужен только отчётный месяц, поэтому чтение
/// останавливается, когда записи не моложе отчётной даты кончились
/// (формат даты в файле — дневной или помесячный).
fn load_ss_rows(
    path: &Path,
    selected_codes: &BTreeSet<i32>,
    report_int: i64,
    report_yyyymm: i64,
) -> Result<Vec<SsRow>> {
    let mut table = DbfTable::open(path)?;
    let fields = SsFields::resolve(&table)?;
    table.par_filter_map_from_end(
        |record| match record.i64(fields.bs1) {
            Some(date) if date >= 10_000_000 => date >= report_int,
            Some(date) => date >= report_yyyymm,
            None => false,
        },
        |record| SsRow::from_dbf(record, &fields, selected_codes),
    )
}

fn load_ppl_rows(path: &Path, selected_codes: &BTreeSet<i32>) -> Result<Vec<PplRow>> {
    let mut table = DbfTable::open(path)?;
    let bbl1 = table.field("BBL1")?;
    let bbl3 = table.field("BBL3")?;
    let bbl4 = table.field("BBL4")?;
    let bl1 = table.field("BL1")?;
    let bl3 = table.field("BL3")?;
    let bl4 = table.field("BL4")?;
    table.par_filter_map(|record| {
        if record.i32(bbl4)? != 1 {
            return None;
        }
        let mest_code = record.i32(bbl1)?;
        if !selected_codes.contains(&mest_code) {
            return None;
        }
        Some(PplRow {
            well: record.well_i64(bbl3)?,
            mest_code,
            date_raw: record.i64(bl1)?,
            rst: record.f64(bl3),
            rpl: record.f64(bl4),
        })
    })
}

impl SsRow {
    /// Дешёвые фильтры (признак BB4, месторождение) — до декодирования
    /// остальных полей.
    fn from_dbf(
        record: &DbfRecordView<'_>,
        fields: &SsFields,
        selected_codes: &BTreeSet<i32>,
    ) -> Option<Self> {
        if record.i32(fields.bb4)? != 1 {
            return None;
        }
        let mest_code = record.i32(fields.bb1)?;
        if !selected_codes.contains(&mest_code) {
            return None;
        }

        Some(Self {
            well: record.well_i64(fields.bb3)?,
            mest_code,
            date_raw: record.i64(fields.bs1)?,
            values: SsLastRow {
                q_work: record.f64(fields.bs21),
                depr: record.f64(fields.bs22),
                rus: record.f64(fields.bs15),
                rzt: record.f64(fields.bs11),
                rshl: record.f64(fields.bs12),
                tus: record.f64(fields.bs16),
                rmk: record.f64(fields.bs19),
                rvh: record.f64(fields.bs13),
                tvh: record.f64(fields.bs17),
                qwat: record.f64(fields.bs23),
            },
        })
    }
}

pub(super) fn build_combined_map_from_last(
    ss_last: &HashMap<i64, SsLastRow>,
    ppl_last: &HashMap<i64, PplLastRow>,
    qdop: Option<&HashMap<i64, f64>>,
    template_needs_pressure_conversion: bool,
) -> HashMap<i64, CombinedRow> {
    let capacity = ss_last.len() + ppl_last.len() + qdop.map_or(0, |values| values.len());
    let mut combined = HashMap::with_capacity(capacity);
    if let Some(values) = qdop {
        for (&well, &value) in values {
            combined
                .entry(well)
                .or_insert_with(CombinedRow::with_ss_defaults)
                .qdop = Some(value);
        }
    }

    for (&well, ppl) in ppl_last {
        let row = combined
            .entry(well)
            .or_insert_with(CombinedRow::with_ss_defaults);
        row.date_key = Some(ppl.date_meas);
        row.rst = ppl.rst;
        row.rpl = ppl.rpl;
    }

    for (&well, ss) in ss_last {
        let row = combined.entry(well).or_default();
        row.q_work = ss.q_work;
        row.depr = ss.depr;
        row.rus = ss.rus;
        row.rzt = ss.rzt;
        row.rshl = ss.rshl;
        row.tus = ss.tus;
        row.rmk = ss.rmk;
        row.rvh = ss.rvh;
        row.tvh = ss.tvh;
        row.qwat = ss.qwat;
    }

    if template_needs_pressure_conversion {
        for row in combined.values_mut() {
            if let Some(value) = row.rst {
                row.rst = Some(value / KGS_CM2_TO_MPA - MPAA_TO_MPAG);
            }
            if let Some(value) = row.rpl {
                row.rpl = Some(value / KGS_CM2_TO_MPA - MPAA_TO_MPAG);
            }
            if let Some(value) = row.depr {
                row.depr = Some(value * MPAA_TO_MPAG);
            }
            row.rus = convert_if_non_zero(row.rus);
            row.rzt = convert_if_non_zero(row.rzt);
            row.rshl = convert_if_non_zero(row.rshl);
            row.rmk = convert_if_non_zero(row.rmk);
            row.rvh = convert_if_non_zero(row.rvh);
        }
    }

    combined
}

/// Пересчёт ата -> МПа изб.; нулевые значения (нет замера) не пересчитываются.
fn convert_if_non_zero(value: Option<f64>) -> Option<f64> {
    value.map(|value| {
        if value.abs() > f64::EPSILON {
            value / KGS_CM2_TO_MPA - MPAA_TO_MPAG
        } else {
            0.0
        }
    })
}
