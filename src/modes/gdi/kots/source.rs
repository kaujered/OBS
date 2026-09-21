//! Excel loading and candidate path resolution for KOTS sources.

use super::*;

use std::{env, fs};

use crate::tabular::sheet::{TextSheet as SheetTable, TextSheetSchema, load_text_sheet};
use crate::tabular::wells::{load_wells_for_mest as load_well_map, row_cell};
use crate::tabular::xlsx::cell_to_display_string as cell_to_table_string;
use crate::tabular::xlsx::{cell_to_f64, cell_to_shared_string};
use ahash::AHashMap as HashMap;

const GDI_DIR: &str = "ГДИ";
const TEMPLATE_DIR: &str = "шаблон";

fn user_home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
}

fn itc_personal_dir() -> Option<PathBuf> {
    user_home_dir().map(|home| {
        home.join("mnt")
            .join("ITC")
            .join("СРМиГРР")
            .join("Персональная")
    })
}

fn bngkm_year_source_dir(year: i32) -> Option<PathBuf> {
    if cfg!(windows) {
        Some(PathBuf::from(format!(r"Y:\ГДИ\БНГКМ\ГДИС {year} года")))
    } else {
        itc_personal_dir().map(|root| {
            root.join("ГДИ")
                .join("БНГКМ")
                .join(format!("ГДИС {year} года"))
        })
    }
}

fn bngkm_kots_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        Some(PathBuf::from(
            r"Y:\ЛАБОРАТОРИЯ МЭМ\3 ДИЗАЙНЕР_СЕТЕЙ\4_БНГКМ\КОЦ_ГДИ БНГКМ",
        ))
    } else {
        itc_personal_dir().map(|root| {
            root.join("ЛАБОРАТОРИЯ МЭМ")
                .join("3 ДИЗАЙНЕР_СЕТЕЙ")
                .join("4_БНГКМ")
                .join("КОЦ_ГДИ БНГКМ")
        })
    }
}

fn bngkm_template_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\GIS\tNavigator_scripts\шаблон")
    } else {
        paths::gis_output_dir(GDI_DIR).join(TEMPLATE_DIR)
    }
}

/// Папка дискового кэша разобранных строк KOTS — внутри папки шаблонов.
pub(super) fn kots_cache_dir() -> PathBuf {
    bngkm_template_dir().join("кэш_ГДИ")
}

pub(super) fn sync_bngkm_kots_sources(current_year: i32) -> Result<()> {
    let source_dir = bngkm_year_source_dir(current_year)
        .ok_or_else(|| anyhow!("Не удалось определить исходную папку KOTS БНГКМ."))?;
    let kots_dir =
        bngkm_kots_dir().ok_or_else(|| anyhow!("Не удалось определить папку КОЦ ГДИ БНГКМ."))?;
    let template_dir = bngkm_template_dir();

    let current_year_file = format!("ГДИ_БНГКМ_{current_year}.xlsm");
    copy_required_file(
        &source_dir.join(&current_year_file),
        &kots_dir.join(&current_year_file),
    )?;
    copy_required_file(
        &source_dir.join("Освоение_ПК.xlsm"),
        &kots_dir.join("Освоение_ПК.xlsm"),
    )?;
    copy_dir_files(&kots_dir, &template_dir)?;

    Ok(())
}

/// Приёмник актуален, если совпадает размер и он не старше источника
/// (fs::copy не переносит mtime, поэтому равенство времён не требуется).
fn needs_copy(source: &Path, target: &Path) -> bool {
    let (Ok(source_meta), Ok(target_meta)) = (fs::metadata(source), fs::metadata(target)) else {
        return true;
    };
    if source_meta.len() != target_meta.len() {
        return true;
    }
    match (source_meta.modified(), target_meta.modified()) {
        (Ok(source_time), Ok(target_time)) => target_time < source_time,
        _ => true,
    }
}

fn copy_required_file(source: &Path, target: &Path) -> Result<()> {
    if !source.is_file() {
        bail!("Не найден исходный файл {}", source.display());
    }
    if !needs_copy(source, target) {
        return Ok(());
    }

    paths::ensure_parent_dir(target)?;
    fs::copy(source, target).with_context(|| {
        format!(
            "Не удалось скопировать {} -> {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn copy_dir_files(source_dir: &Path, target_dir: &Path) -> Result<()> {
    fs::create_dir_all(target_dir)
        .with_context(|| format!("Не удалось создать папку {}", target_dir.display()))?;

    let entries = fs::read_dir(source_dir)
        .with_context(|| format!("Не удалось прочитать папку {}", source_dir.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("Не удалось прочитать элемент в {}", source_dir.display()))?;
        let source = entry.path();
        if !source.is_file() {
            continue;
        }

        let target = target_dir.join(entry.file_name());
        if !needs_copy(&source, &target) {
            continue;
        }
        fs::copy(&source, &target).with_context(|| {
            format!(
                "Не удалось скопировать {} -> {}",
                source.display(),
                target.display()
            )
        })?;
    }

    Ok(())
}

pub(super) fn load_wells(path: &Path, mest: Mest) -> Result<HashMap<i64, WellRow>> {
    Ok(load_well_map(path, mest, |headers, row| {
        let well = row_cell(headers, row, "well").and_then(cell_to_i64)?;
        Some((
            well,
            WellRow {
                plast: row_cell(headers, row, "plast").and_then(cell_to_shared_string),
                gp: row_cell(headers, row, "gp").and_then(cell_to_shared_string),
                depr: row_cell(headers, row, "depr").and_then(cell_to_f64),
                tpl: row_cell(headers, row, "tpl").and_then(cell_to_f64),
                gauge: row_cell(headers, row, "gauge").and_then(cell_to_f64),
            },
        ))
    })?
    .into_iter()
    .collect())
}

pub(super) fn load_sheet_table(
    path: &Path,
    sheet_name: &str,
    skip_rows: usize,
    stop_header: &str,
) -> Result<SheetTable> {
    load_text_sheet(
        path,
        sheet_name,
        TextSheetSchema::new(skip_rows, stop_header),
    )
}

pub(super) fn first_existing_file(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_file()).cloned()
}

pub(super) fn required_existing(candidates: &[PathBuf], name: &str) -> Result<PathBuf> {
    first_existing_file(candidates).ok_or_else(|| {
        anyhow!(
            "Не найден {}. Проверены пути:\n{}",
            name,
            paths::format_candidate_paths(candidates)
        )
    })
}

fn extend_debug_candidates(out: &mut Vec<PathBuf>, file_names: &[&str]) {
    let debug_root = PathBuf::from(paths::DEBUG_DIR);
    let debug_data_root = debug_root.join("Данные для внесения в БД");
    for root in [debug_root, debug_data_root] {
        for file_name in file_names {
            out.push(root.join(file_name));
        }
    }
}

pub(super) fn bngkm_year_candidates(year: i32, debug_mode: bool) -> Vec<PathBuf> {
    let legacy_file = format!("ГДИ_БНГКМ_{year}.xlsm");
    let debug_file = format!("ГДИ БНГКМ {year}.xlsm");
    let mut out = Vec::new();
    if debug_mode {
        extend_debug_candidates(&mut out, &[debug_file.as_str(), legacy_file.as_str()]);
    }
    out.extend([
        bngkm_template_dir().join(&legacy_file),
        paths::gis_output_dir(GDI_DIR).join(&legacy_file),
        paths::gis_output_dir(GDI_DIR).join(&debug_file),
    ]);
    if let Some(kots_dir) = bngkm_kots_dir() {
        out.push(kots_dir.join(&legacy_file));
    }
    if year == Local::now().year()
        && let Some(source_dir) = bngkm_year_source_dir(year)
    {
        out.push(source_dir.join(&legacy_file));
    }
    out
}

pub(super) fn bngkm_pk_candidates(debug_mode: bool) -> Vec<PathBuf> {
    let files = ["Освоение_ПК.xlsm", "Освоение ПК.xlsm"];
    let mut out = Vec::new();
    if debug_mode {
        extend_debug_candidates(&mut out, &files);
    }
    for file in files {
        out.push(bngkm_template_dir().join(file));
        out.push(paths::gis_output_dir(GDI_DIR).join(file));
        if let Some(kots_dir) = bngkm_kots_dir() {
            out.push(kots_dir.join(file));
        }
    }
    out
}

pub(super) fn bngkm_gp3_candidates(debug_mode: bool) -> Vec<PathBuf> {
    let files = ["Освоение ГП-3.xlsx", "Освоение_ГП-3.xlsx"];
    let mut out = Vec::new();
    if debug_mode {
        extend_debug_candidates(&mut out, &files);
    }
    for file in files {
        out.push(bngkm_template_dir().join(file));
        out.push(paths::gis_output_dir(GDI_DIR).join(file));
        if let Some(kots_dir) = bngkm_kots_dir() {
            out.push(kots_dir.join(file));
        }
    }
    out
}

pub(super) fn hgkm_candidates(debug_mode: bool) -> Vec<PathBuf> {
    let files = [
        "Освоение ХГКМ.xlsm",
        "Освоение ХГКМ.xlsx",
        "Освоение_ХГКМ.xlsm",
        "Освоение_ХГКМ.xlsx",
    ];
    let mut out = Vec::new();
    if debug_mode {
        extend_debug_candidates(&mut out, &files);
    }
    for file in files {
        out.push(paths::gis_output_dir(GDI_DIR).join(TEMPLATE_DIR).join(file));
        out.push(paths::gis_output_dir(GDI_DIR).join(file));
        if cfg!(windows) {
            out.push(PathBuf::from(r"Y:\ГДИ\ХГКМ\Освоение").join(file));
        } else if let Some(root) = itc_personal_dir() {
            out.push(root.join("ГДИ").join("ХГКМ").join("Освоение").join(file));
        }
    }
    out
}

pub(super) fn cell_to_i64(cell: &Data) -> Option<i64> {
    crate::tabular::xlsx::cell_to_i64(cell).or_else(|| normalize_well(&cell_to_table_string(cell)))
}
