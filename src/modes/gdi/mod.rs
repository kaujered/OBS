//! The `GDI` report.
//!
//! The report has two branches:
//! - standard DBF processing implemented here;
//! - KOTS/Excel compatibility processing delegated to `kots`.
//!
//! `limits` holds the limit calculation shared by both branches.

mod data;
mod kots;
mod limits;
mod source;
mod writer;

pub use data::{DataSourceChoice, Request};

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ahash::AHashMap as HashMap;
use anyhow::{Context, Result, bail};
use rayon::prelude::*;

use self::limits::{apply_limit_results, retain_last_date_per_well};
use crate::domain::Mest;
use crate::paths;

use self::data::{
    KsmestLast, OutputRow, PlastLast, Stand1Last, Stand2Row, WellRow, cmp_opt_f64, mean_non_zero,
};
use self::source::{
    load_ksmest_last, load_plast_last, load_stand1_last, load_stand2_rows, load_wells_for_mests,
    resolve_wells_path,
};
use self::writer::write_workbook;
use crate::tabular::cache::{SourceCache, file_stamp};

type FileStamp = Option<(std::time::SystemTime, u64)>;
type GdiCacheKey = ([FileStamp; 4], BTreeSet<i32>, i64, bool);

/// Результаты разбора четырёх DBF-источников GDI.
struct GdiTables {
    stand2_by_mest: HashMap<i32, Vec<Stand2Row>>,
    ksmest_by_mest: HashMap<i32, HashMap<i64, KsmestLast>>,
    plast_by_mest: HashMap<i32, HashMap<i64, PlastLast>>,
    stand1_by_mest: HashMap<i32, HashMap<i64, Stand1Last>>,
}

static TABLES_CACHE: SourceCache<GdiCacheKey, GdiTables> = SourceCache::new();

pub fn execute(request: &Request) -> Result<Vec<PathBuf>> {
    if request.mests.is_empty() {
        bail!("Выберите хотя бы одно месторождение.");
    }

    // Split the request early so DBF logic stays independent from the KOTS fallback branch.
    let mut dbf_request = request.clone();
    dbf_request.mests = request
        .mests
        .iter()
        .copied()
        .filter(|mest| {
            !((mest.is_bngkm() && request.bngkm_source == DataSourceChoice::Kots)
                || (mest.is_hgkm() && request.hgkm_source == DataSourceChoice::Kots))
        })
        .collect();

    let run_bngkm_kots = request
        .mests
        .iter()
        .any(|mest| mest.is_bngkm() && request.bngkm_source == DataSourceChoice::Kots);
    let run_hgkm_kots = request
        .mests
        .iter()
        .any(|mest| mest.is_hgkm() && request.hgkm_source == DataSourceChoice::Kots);
    let kots_wells_path = (run_bngkm_kots || run_hgkm_kots)
        .then(|| resolve_wells_path(request.debug_mode))
        .transpose()?;
    let kots_wells = kots_wells_path.as_deref();
    type KotsExecute = fn(i32, u32, u32, &Path, bool, bool, Option<i64>) -> Result<PathBuf>;
    let run_kots = |run: bool, execute: KotsExecute| -> Result<Option<PathBuf>> {
        if !run {
            return Ok(None);
        }
        let wells_path = kots_wells
            .ok_or_else(|| anyhow::anyhow!("Не найден СкважиныГДН.xlsx для KOTS-ветки GDI"))?;
        execute(
            request.year,
            request.month,
            request.day,
            wells_path,
            request.debug_mode,
            request.export_charts,
            request.filter_date,
        )
        .map(Some)
    };

    // Три ветки независимы: DBF-месторождения, БНГКМ-КОЦ и ХГКМ-КОЦ
    // выполняются параллельно.
    let (dbf_created, (bngkm_created, hgkm_created)) = rayon::join(
        || -> Result<Vec<PathBuf>> {
            if dbf_request.mests.is_empty() {
                return Ok(Vec::new());
            }
            execute_dbf(&dbf_request)
        },
        || {
            rayon::join(
                || run_kots(run_bngkm_kots, kots::execute_bngkm),
                || run_kots(run_hgkm_kots, kots::execute_hgkm),
            )
        },
    );

    let mut created = dbf_created?;
    created.extend(bngkm_created?);
    created.extend(hgkm_created?);

    if request.debug_mode {
        return Ok(created);
    }

    paths::replicate_output_files(
        &created,
        &paths::gis_output_dir("ГДИ"),
        paths::OutputGroup::Gdi,
    )
}

fn execute_dbf(request: &Request) -> Result<Vec<PathBuf>> {
    if request.mests.is_empty() {
        bail!("Выберите хотя бы одно месторождение.");
    }

    if request.mests.iter().any(|mest| {
        (mest.is_bngkm() && request.bngkm_source == DataSourceChoice::Kots)
            || (mest.is_hgkm() && request.hgkm_source == DataSourceChoice::Kots)
    }) {
        bail!(
            "KOTS-ветки GDI остаются в режиме совместимости Python. Для нативного запуска оставьте источник DBF."
        );
    }

    let paths = paths::resolve_gdi_paths(request.test_mode, request.debug_mode)?;
    // МГПУ absorbs Ныда: load the source fields so the merged mest1.xlsx has both,
    // while a separately selected Ныда still gets its own mest7.xlsx.
    let load_mests = request
        .mests
        .iter()
        .flat_map(|mest| mest.merge_sources().iter().copied())
        .collect::<BTreeSet<_>>();
    let wells_by_mest = load_wells_for_mests(
        &paths.wells_xlsx,
        &load_mests.iter().copied().collect::<Vec<_>>(),
    )?;
    let selected_codes = load_mests
        .iter()
        .map(|mest| mest.code())
        .collect::<BTreeSet<_>>();
    let date_filter =
        i64::from(request.year) * 10_000 + i64::from(request.month) * 100 + i64::from(request.day);

    let cache_key = (
        [
            file_stamp(&paths.stand2_dbf),
            file_stamp(&paths.ksmest_dbf),
            file_stamp(&paths.plast_dbf),
            file_stamp(&paths.stand1_dbf),
        ],
        selected_codes.clone(),
        date_filter,
        request.include_rejected,
    );
    let tables = TABLES_CACHE.get_or_load(cache_key, || {
        // These DBF files are independent, so parsing them in parallel reduces startup latency.
        let ((stand2_by_mest, ksmest_by_mest), (plast_by_mest, stand1_by_mest)) = rayon::join(
            || {
                rayon::join(
                    || {
                        load_stand2_rows(
                            &paths.stand2_dbf,
                            &selected_codes,
                            date_filter,
                            request.include_rejected,
                        )
                        .with_context(|| {
                            format!("Не удалось прочитать {}", paths.stand2_dbf.display())
                        })
                    },
                    || {
                        load_ksmest_last(&paths.ksmest_dbf, &selected_codes).with_context(|| {
                            format!("Не удалось прочитать {}", paths.ksmest_dbf.display())
                        })
                    },
                )
            },
            || {
                rayon::join(
                    || {
                        load_plast_last(&paths.plast_dbf, &selected_codes).with_context(|| {
                            format!("Не удалось прочитать {}", paths.plast_dbf.display())
                        })
                    },
                    || {
                        load_stand1_last(&paths.stand1_dbf, &selected_codes).with_context(|| {
                            format!("Не удалось прочитать {}", paths.stand1_dbf.display())
                        })
                    },
                )
            },
        );
        Ok(GdiTables {
            stand2_by_mest: stand2_by_mest?,
            ksmest_by_mest: ksmest_by_mest?,
            plast_by_mest: plast_by_mest?,
            stand1_by_mest: stand1_by_mest?,
        })
    })?;
    let GdiTables {
        stand2_by_mest,
        ksmest_by_mest,
        plast_by_mest,
        stand1_by_mest,
    } = &*tables;

    let jobs = request
        .mests
        .iter()
        .map(|mest| {
            // Merge the field with its sources (МГПУ + Ныда) into one workbook.
            let mut job = MestJob {
                mest: *mest,
                wells: HashMap::new(),
                stand2_rows: Vec::new(),
                ksmest_last: HashMap::new(),
                plast_last: HashMap::new(),
                stand1_last: HashMap::new(),
            };
            for src in mest.merge_sources() {
                let code = src.code();
                let src_wells = wells_by_mest.get(src).ok_or_else(|| {
                    anyhow::anyhow!("Не найдены данные СкважиныГДН.xlsx для {}", src.ui_label())
                })?;
                job.wells
                    .extend(src_wells.iter().map(|(well, row)| (*well, row.clone())));
                if let Some(rows) = stand2_by_mest.get(&code) {
                    job.stand2_rows.extend(rows.iter().cloned());
                }
                if let Some(map) = ksmest_by_mest.get(&code) {
                    job.ksmest_last
                        .extend(map.iter().map(|(well, value)| (*well, value.clone())));
                }
                if let Some(map) = plast_by_mest.get(&code) {
                    job.plast_last
                        .extend(map.iter().map(|(well, value)| (*well, value.clone())));
                }
                if let Some(map) = stand1_by_mest.get(&code) {
                    job.stand1_last
                        .extend(map.iter().map(|(well, value)| (*well, value.clone())));
                }
            }
            Ok(job)
        })
        .collect::<Result<Vec<_>>>()?;

    jobs.into_par_iter()
        .map(|job| process_mest(request, job))
        .collect()
}

/// Входные данные одного месторождения (источники МГПУ + Ныда уже слиты).
struct MestJob {
    mest: Mest,
    wells: HashMap<i64, WellRow>,
    stand2_rows: Vec<Stand2Row>,
    ksmest_last: HashMap<i64, KsmestLast>,
    plast_last: HashMap<i64, PlastLast>,
    stand1_last: HashMap<i64, Stand1Last>,
}

fn process_mest(request: &Request, job: MestJob) -> Result<PathBuf> {
    let MestJob {
        mest,
        wells,
        stand2_rows,
        ksmest_last,
        plast_last,
        stand1_last,
    } = job;
    let mut rows = Vec::with_capacity(stand2_rows.len());
    for row in stand2_rows {
        let well_meta = wells.get(&row.well);
        let gauge = ksmest_last
            .get(&row.well)
            .and_then(|value| mean_non_zero(&value.gauge_values));
        let tpl = plast_last
            .get(&row.well)
            .and_then(|value| value.tpl_kelvin)
            .map(|value| value - 273.15);
        let dep = match (row.ppl, row.bhp) {
            (Some(ppl), Some(bhp)) => Some(ppl - bhp),
            _ => None,
        };

        rows.push(OutputRow {
            gp: well_meta.and_then(|value| value.gp.clone()),
            plast: well_meta.and_then(|value| value.plast.clone()),
            regime_no: row.regime_no,
            washer: row.washer,
            well: row.well,
            date_key: row.date_key,
            thp: row.thp,
            flo: row.flo,
            gauge,
            bhp: row.bhp,
            pst: row.pst,
            ppl: row.ppl,
            tpl,
            water: row.water,
            sand: row.sand,
            dep,
            max_dep: well_meta.and_then(|value| value.depr),
            c: stand1_last.get(&row.well).and_then(|value| value.c),
            n: stand1_last.get(&row.well).and_then(|value| value.n),
            max_flo: None,
            dop_skv_flo: None,
            dop_skv_flo_percent_5: None,
            dop_skv_flo_095: None,
            limit_comment: None,
            jones_a: None,
            jones_b: None,
        });
    }

    rows.sort_by(|left, right| {
        left.well
            .cmp(&right.well)
            .then(left.date_key.cmp(&right.date_key))
            .then(cmp_opt_f64(left.regime_no, right.regime_no))
    });

    if request.only_last {
        retain_last_date_per_well(&mut rows, |row| (row.well, row.date_key));
    }

    let output_path = paths::report_output_dir("ГДИ", "ГДИ", request.debug_mode)
        .join(format!("mest{}.xlsx", mest.code()));
    apply_limit_results(&mut rows);
    write_workbook(
        &output_path,
        &rows,
        request.export_charts,
        request.filter_date,
    )?;
    Ok(output_path)
}
