//! Native implementation of the `VED` report.
//!
//! Layout mirrors the other processing modes:
//! - `data` stores report rows and pure merge helpers;
//! - `source` loads DBF/XLSX inputs and parsing utilities;
//! - `writer` fills Excel templates and saves the final workbook.

mod data;
mod sheet_xml;
mod source;
mod writer;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use calamine::Data;
use chrono::{Duration, NaiveDate};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use regex::Regex;

use crate::domain::Mest;
use crate::paths;

use self::data::*;
use self::source::*;
use self::writer::*;

const KGS_CM2_TO_MPA: f64 = 10.197;
const MPAA_TO_MPAG: f64 = 0.101_325;
const TITLE_SEARCH: &str = "ТЕХНОЛОГИЧЕСКИЙ РЕЖИМ СКВАЖИН ГАЗОВОГО ПРОМЫСЛА НА";

#[derive(Debug, Clone)]
pub struct Request {
    pub mests: Vec<Mest>,
    pub year: i32,
    pub month: u32,
    pub test_mode: bool,
    pub debug_mode: bool,
    /// Согласовывать Депр/Рус при рабочем дебите с оптимальным и допустимым
    /// режимами промысла (по умолчанию включено).
    pub correct_work_params: bool,
}

pub fn execute(request: &Request) -> Result<Vec<PathBuf>> {
    if request.mests.is_empty() {
        bail!("Выберите хотя бы одно месторождение.");
    }

    let report_date = report_date_from_year_month(request.year, request.month)?;
    let (report_int, report_yyyymm) = parse_report_date(&report_date)?;
    let paths = paths::resolve_ved_paths(request.test_mode, request.debug_mode)?;

    let templates_all = scan_templates_folder(&paths.templates_dir)?;
    if templates_all.is_empty() {
        bail!(
            "В папке шаблонов не найдено .xlsm/.xlsx: {}",
            paths.templates_dir.display()
        );
    }

    // МГПУ also emits the separate Ныда ведомость (its own file), so expand the
    // selection to the source fields (ГП1…ГП9 share the МГПУ label; Ныда adds its own).
    let effective_mests = request
        .mests
        .iter()
        .flat_map(|mest| mest.merge_sources().iter().copied())
        .collect::<BTreeSet<_>>();
    let selected_labels = effective_mests
        .iter()
        .map(|mest| ved_label(*mest))
        .collect::<BTreeSet<_>>();
    let selected_codes = effective_mests
        .iter()
        .map(|mest| mest.code())
        .collect::<BTreeSet<_>>();
    let store = DataStore::load_cached(&paths, &selected_codes, report_int, report_yyyymm)?;

    let mut grouped = BTreeMap::<(Option<i32>, bool), Vec<PathBuf>>::new();
    for template in templates_all {
        if !selected_labels.contains(template.mest_label) {
            continue;
        }
        grouped
            .entry((
                template.mest_code,
                matches!(template.mest_code, Some(4 | 5)),
            ))
            .or_default()
            .push(template.path);
    }
    if grouped.is_empty() {
        bail!("Не найдено шаблонов для выбранных месторождений.");
    }

    let created = grouped
        .into_par_iter()
        .try_fold(
            Vec::new,
            |mut generated, ((mest_code, template_needs_pressure_conversion), templates)| {
                let ss_last = store.ss_last_by_well(mest_code, report_int, report_yyyymm);
                let ppl_last = store.ppl_last_by_well(mest_code, report_int);
                let qdop = store.qdop_for_mest(mest_code);
                let combined = build_combined_map_from_last(
                    &ss_last,
                    &ppl_last,
                    qdop,
                    template_needs_pressure_conversion,
                );

                // Шаблоны группы независимы: заполняются параллельно,
                // общая карта данных только читается.
                let filled = templates
                    .into_par_iter()
                    .map(|template| {
                        let regime = match paths.regime_dir.as_deref() {
                            Some(dir) => load_regime_for_template(dir, &template)?,
                            None => None,
                        };
                        let options = FillOptions {
                            regime: regime.as_ref(),
                            pressure_conversion: template_needs_pressure_conversion,
                            correct_work_params: request.correct_work_params,
                        };
                        fill_template(
                            &template,
                            &paths.output_dir,
                            &combined,
                            &options,
                            request.year,
                            request.month,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                generated.extend(filled);

                Ok::<Vec<PathBuf>, anyhow::Error>(generated)
            },
        )
        .try_reduce(Vec::new, |mut left, mut right| {
            left.append(&mut right);
            Ok(left)
        })?;

    if request.debug_mode {
        return Ok(created);
    }

    paths::replicate_output_files(&created, &paths.output_dir, paths::OutputGroup::Vedomost)
}
