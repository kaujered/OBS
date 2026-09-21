//! Shared filesystem resolution helpers for path lookup and output creation.

use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::candidates::{
    DEBUG_DIR, bdasu_dir_candidates, debug_bdasu_file, medbegie_dir_candidates,
};

pub(super) fn home_dir() -> Result<PathBuf> {
    if let Some(v) = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")) {
        return Ok(PathBuf::from(v));
    }
    bail!("Не удалось определить домашнюю папку пользователя.")
}

pub(super) fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.is_file()).cloned()
}

pub(super) fn first_existing_dir(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.is_dir()).cloned()
}

pub(crate) fn format_candidate_paths(candidates: &[PathBuf]) -> String {
    let mut rendered = String::new();
    for path in candidates {
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        let _ = write!(rendered, "{}", path.display());
    }
    rendered
}

pub(super) fn resolve_existing(name: &str, candidates: &[PathBuf]) -> Result<PathBuf> {
    if let Some(path) = first_existing(candidates) {
        return Ok(path);
    }

    let rendered = format_candidate_paths(candidates);
    bail!("Не найден {name}. Проверены пути:\n{rendered}");
}

pub(super) fn resolve_existing_dir(name: &str, candidates: &[PathBuf]) -> Result<PathBuf> {
    if let Some(path) = first_existing_dir(candidates) {
        return Ok(path);
    }

    let rendered = format_candidate_paths(candidates);
    bail!("Не найдена папка {name}. Проверены пути:\n{rendered}");
}

pub(super) fn resolve_dbf(name: &str, test_mode: bool, debug_mode: bool) -> Result<PathBuf> {
    if debug_mode {
        return resolve_existing(name, &[debug_bdasu_file(name)]);
    }

    // Search order matters: local/primary paths first, then shared network mirrors.
    let mut candidates = medbegie_dir_candidates(test_mode, name)?;
    candidates.extend(bdasu_dir_candidates(test_mode, name)?);
    resolve_existing(name, &candidates)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputGroup {
    Vedomost,
    RegimeSheet,
    Gdi,
    Statika,
    DailySummary,
}

pub fn gis_output_dir(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\GIS").join(name)
    } else if let Ok(home) = home_dir() {
        home.join("tNavigator_scripts").join(name)
    } else {
        PathBuf::from(".").join(name)
    }
}

/// Output directory for a report. In debug mode everything is written under
/// `DEBUG_DIR/<debug_name>` (the production folder may carry a different name).
pub fn report_output_dir(prod_name: &str, debug_name: &str, debug_mode: bool) -> PathBuf {
    if debug_mode {
        return PathBuf::from(DEBUG_DIR).join(debug_name);
    }
    gis_output_dir(prod_name)
}

pub fn replicate_output_files(
    primary_paths: &[PathBuf],
    primary_root: &Path,
    group: OutputGroup,
) -> Result<Vec<PathBuf>> {
    let Some(mirror_root) = available_additional_output_dir(group) else {
        return Ok(primary_paths.to_vec());
    };
    if mirror_root.as_path() == primary_root {
        return Ok(primary_paths.to_vec());
    }

    copy_outputs_to_dir(primary_paths, primary_root, &mirror_root)
}

pub fn preferred_result_dirs(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = BTreeSet::new();

    for path in paths {
        let Some(parent) = path.parent() else {
            continue;
        };
        dirs.insert(preferred_result_dir(parent));
    }

    dirs.into_iter().collect()
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        // Writers call this once before saving so mode code can focus on report logic.
        fs::create_dir_all(parent)
            .with_context(|| format!("Не удалось создать папку {}", parent.display()))?;
    }
    Ok(())
}

/// Общая папка «5 БД» с готовыми выгрузками — по подкаталогу на режим.
fn shared_db_root() -> Option<PathBuf> {
    if cfg!(windows) {
        Some(PathBuf::from(r"Y:\ЛАБОРАТОРИЯ МЭМ\5 БД"))
    } else {
        Some(
            home_dir()
                .ok()?
                .join("mnt")
                .join("ITC")
                .join("СРМиГРР")
                .join("Персональная")
                .join("ЛАБОРАТОРИЯ МЭМ")
                .join("5 БД"),
        )
    }
}

/// Подкаталог «5 БД» по имени — для режимов, которые пишут туда напрямую, а не
/// зеркалят туда результат из своей рабочей папки.
pub fn shared_db_dir(name: &str) -> Option<PathBuf> {
    Some(shared_db_root()?.join(name))
}

pub(super) fn available_additional_output_dir(group: OutputGroup) -> Option<PathBuf> {
    let candidate = shared_db_root()?.join(match group {
        OutputGroup::Vedomost => "0_ВЕДОМОСТЬ",
        OutputGroup::RegimeSheet => "1_РЕЖИМНЫЙ_ЛИСТ",
        OutputGroup::Gdi => "2_ГДИ",
        OutputGroup::Statika => "3_СТАТИКА",
        OutputGroup::DailySummary => "4_СУТОЧНЫЕ_СВОДКИ",
    });

    candidate.is_dir().then_some(candidate)
}

fn preferred_result_dir(dir: &Path) -> PathBuf {
    for group in [
        OutputGroup::Vedomost,
        OutputGroup::RegimeSheet,
        OutputGroup::Gdi,
        OutputGroup::Statika,
        OutputGroup::DailySummary,
    ] {
        if let Some(preferred) = preferred_result_dir_for_group(dir, group) {
            return preferred;
        }
    }

    dir.to_path_buf()
}

fn preferred_result_dir_for_group(dir: &Path, group: OutputGroup) -> Option<PathBuf> {
    let primary_roots = primary_output_roots(group);
    let mirror_root = available_additional_output_dir(group);
    select_preferred_dir(dir, &primary_roots, mirror_root.as_deref())
}

fn primary_output_roots(group: OutputGroup) -> Vec<PathBuf> {
    if cfg!(windows) {
        match group {
            OutputGroup::Vedomost => vec![
                PathBuf::from(r"C:\GIS\Ведомость"),
                PathBuf::from(r"C:\GIS\Ведомости техрежима"),
            ],
            OutputGroup::RegimeSheet => {
                vec![PathBuf::from(
                    r"Y:\ЛАБОРАТОРИЯ МЭМ\2 Контроль тех режимов\сводки_БНГКМ",
                )]
            }
            OutputGroup::Gdi => vec![gis_output_dir("ГДИ")],
            OutputGroup::Statika => vec![gis_output_dir("Статика")],
            OutputGroup::DailySummary => vec![gis_output_dir("Суточные сводки")],
        }
    } else {
        let Some(home) = home_dir().ok() else {
            return Vec::new();
        };

        match group {
            OutputGroup::Vedomost => vec![
                home.join("tNavigator_scripts").join("Ведомость"),
                home.join("tNavigator_scripts").join("Ведомости техрежима"),
            ],
            OutputGroup::RegimeSheet => vec![
                home.join("mnt")
                    .join("ITC")
                    .join("СРМиГРР")
                    .join("Персональная")
                    .join("ЛАБОРАТОРИЯ МЭМ")
                    .join("2 Контроль тех режимов")
                    .join("сводки_БНГКМ"),
            ],
            OutputGroup::Gdi => vec![gis_output_dir("ГДИ")],
            OutputGroup::Statika => vec![gis_output_dir("Статика")],
            OutputGroup::DailySummary => vec![gis_output_dir("Суточные сводки")],
        }
    }
}

fn select_preferred_dir(
    dir: &Path,
    primary_roots: &[PathBuf],
    mirror_root: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(mirror_root) = mirror_root {
        if let Ok(relative) = dir.strip_prefix(mirror_root) {
            if dir.is_dir() {
                return Some(dir.to_path_buf());
            }

            for primary_root in primary_roots {
                let candidate = primary_root.join(relative);
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }

            return None;
        }
    }

    for primary_root in primary_roots {
        if let Ok(relative) = dir.strip_prefix(primary_root) {
            if let Some(mirror_root) = mirror_root {
                let candidate = mirror_root.join(relative);
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }

            if dir.is_dir() {
                return Some(dir.to_path_buf());
            }

            for fallback_root in primary_roots {
                let candidate = fallback_root.join(relative);
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }

            return None;
        }
    }

    None
}

fn copy_outputs_to_dir(
    primary_paths: &[PathBuf],
    primary_root: &Path,
    mirror_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut created = Vec::with_capacity(primary_paths.len() * 2);

    for primary_path in primary_paths {
        created.push(primary_path.clone());

        let relative_path = primary_path.strip_prefix(primary_root).with_context(|| {
            format!(
                "Не удалось определить относительный путь для {} от {}",
                primary_path.display(),
                primary_root.display()
            )
        })?;
        let mirror_path = mirror_root.join(relative_path);
        if mirror_path == *primary_path {
            continue;
        }

        ensure_parent_dir(&mirror_path)?;
        fs::copy(primary_path, &mirror_path).with_context(|| {
            format!(
                "Не удалось скопировать итоговый файл {} -> {}",
                primary_path.display(),
                mirror_path.display()
            )
        })?;
        created.push(mirror_path);
    }

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn copy_outputs_to_dir_preserves_relative_layout() {
        let root = unique_temp_dir("mirror_outputs");
        let primary_root = root.join("primary");
        let mirror_root = root.join("mirror");
        let primary_year_dir = primary_root.join("2026");
        let primary_nested_dir = primary_root.join("nested");

        fs::create_dir_all(&primary_year_dir).unwrap();
        fs::create_dir_all(&primary_nested_dir).unwrap();
        fs::create_dir_all(&mirror_root).unwrap();

        let first = primary_year_dir.join("report_a.xlsx");
        let second = primary_nested_dir.join("report_b.xlsx");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();

        let created = copy_outputs_to_dir(
            &[first.clone(), second.clone()],
            &primary_root,
            &mirror_root,
        )
        .unwrap();

        let mirrored_first = mirror_root.join("2026").join("report_a.xlsx");
        let mirrored_second = mirror_root.join("nested").join("report_b.xlsx");
        assert_eq!(
            created,
            vec![
                first.clone(),
                mirrored_first.clone(),
                second.clone(),
                mirrored_second.clone()
            ]
        );
        assert_eq!(fs::read(&mirrored_first).unwrap(), b"first");
        assert_eq!(fs::read(&mirrored_second).unwrap(), b"second");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn select_preferred_dir_uses_mirror_when_available() {
        let root = unique_temp_dir("preferred_dir_mirror");
        let primary_root = root.join("primary");
        let mirror_root = root.join("mirror");
        let primary_dir = primary_root.join("2026");
        let mirror_dir = mirror_root.join("2026");

        fs::create_dir_all(&primary_dir).unwrap();
        fs::create_dir_all(&mirror_dir).unwrap();

        let selected = select_preferred_dir(
            &primary_dir,
            std::slice::from_ref(&primary_root),
            Some(&mirror_root),
        );
        assert_eq!(selected, Some(mirror_dir));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn select_preferred_dir_falls_back_to_primary_when_mirror_missing() {
        let root = unique_temp_dir("preferred_dir_primary");
        let primary_root = root.join("primary");
        let mirror_root = root.join("mirror");
        let primary_dir = primary_root.join("2026");

        fs::create_dir_all(&primary_dir).unwrap();

        let selected = select_preferred_dir(
            &primary_dir,
            std::slice::from_ref(&primary_root),
            Some(&mirror_root),
        );
        assert_eq!(selected, Some(primary_dir));

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{unique}"))
    }
}
