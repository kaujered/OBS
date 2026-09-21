//! Native implementation of the `Телеметрия` copier.
//!
//! Walks the daily summary folders of the selected year on the network share
//! (`<год>/<месяц>/<дд.мм.гг>`) and copies every `Параметры работы скважин*.xls*`
//! into `5 БД/5_ТЕЛЕМЕТРИЯ/<год>`. Months are scanned in parallel because each
//! directory listing is a network round-trip.
//!
//! Every copy is named `Параметры_работы_скважин_БНГКМ_ГГГГММДД.xlsm`, whatever
//! the source file was called.
//!
//! With `continue_from_last` the run starts at the newest date already present in
//! the destination instead of the beginning of the year.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate};
use once_cell::sync::Lazy;
use rayon::prelude::*;

use crate::paths;

/// Имя, под которым сводка лежит почти всегда. Точечная проверка по нему
/// дешевле листинга всей дневной папки, поэтому обход по маске включается
/// только когда этого файла нет.
const PRIMARY_SOURCE_FILE: &str = "Параметры работы скважин БНГКМ.xlsm";

/// Маска исходных файлов: `Параметры работы скважин*.xls*`. Суффикс после
/// «скважин» у разных месторождений свой, а расширение бывает `xls`, `xlsx` и
/// `xlsm`, поэтому имя проверяется по началу и по расширению, а не целиком.
const SOURCE_FILE_PREFIX: &str = "Параметры работы скважин";
const SOURCE_FILE_EXT_PREFIX: &str = "xls";

/// Имя копии в папке назначения. Одинаковое для всех источников: за день
/// выгружается одна сводка, и в отчётной папке она должна называться одинаково,
/// как бы файл ни назывался на шаре.
const TARGET_FILE_STEM: &str = "Параметры_работы_скважин_БНГКМ";
const TARGET_FILE_EXT: &str = "xlsm";
const MONTH_NAMES: [&str; 12] = [
    "Январь",
    "Февраль",
    "Март",
    "Апрель",
    "Май",
    "Июнь",
    "Июль",
    "Август",
    "Сентябрь",
    "Октябрь",
    "Ноябрь",
    "Декабрь",
];
static MONTH_NAMES_LC: Lazy<Vec<String>> =
    Lazy::new(|| MONTH_NAMES.iter().map(|name| name.to_lowercase()).collect());

#[derive(Debug, Clone)]
pub struct Request {
    pub year: i32,
    /// Продолжить с последней сводки, уже лежащей в папке выгрузки, вместо
    /// обхода года с начала.
    pub continue_from_last: bool,
}

#[derive(Debug, Clone)]
struct CopyJob {
    source: PathBuf,
    target: PathBuf,
}

pub fn execute(request: &Request) -> Result<Vec<PathBuf>> {
    if request.year < 1970 {
        bail!("Год должен быть не меньше 1970.");
    }

    let paths = paths::resolve_telemetry_paths(request.year)?;
    let output_year_dir = paths.output_dir.join(request.year.to_string());
    fs::create_dir_all(&output_year_dir)
        .with_context(|| format!("Не удалось создать папку {}", output_year_dir.display()))?;

    let since = if request.continue_from_last {
        last_exported_date(&output_year_dir)?
    } else {
        None
    };

    let jobs = discover_copy_jobs(&paths.source_dir, &output_year_dir, request.year, since)?;
    if jobs.is_empty() {
        match since {
            Some(date) => bail!(
                "Нет сводок начиная с {} в {}.",
                date.format("%d.%m.%Y"),
                paths.source_dir.display()
            ),
            None => bail!(
                "Не найдено файлов `{SOURCE_FILE_PREFIX}*.{SOURCE_FILE_EXT_PREFIX}*` в {} за {} год.",
                paths.source_dir.display(),
                request.year
            ),
        }
    }

    let mut created = jobs.par_iter().map(copy_job).collect::<Result<Vec<_>>>()?;
    created.sort();
    Ok(created)
}

/// Самая поздняя дата среди уже выгруженных сводок — с неё продолжается обход.
/// Пустая папка означает, что продолжать не с чего и год берётся целиком.
fn last_exported_date(output_dir: &Path) -> Result<Option<NaiveDate>> {
    let entries = fs::read_dir(output_dir)
        .with_context(|| format!("Не удалось прочитать папку {}", output_dir.display()))?;

    let mut latest = None;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("Не удалось прочитать элемент в {}", output_dir.display()))?;
        let file_name = entry.file_name();
        let Some(date) = file_name.to_str().and_then(exported_date) else {
            continue;
        };
        latest = latest.max(Some(date));
    }

    Ok(latest)
}

/// Дата из имени готовой копии — суффикс `_ГГГГММДД` перед расширением.
fn exported_date(file_name: &str) -> Option<NaiveDate> {
    let stem = Path::new(file_name).file_stem()?.to_str()?;
    let (_, tail) = stem.rsplit_once('_')?;

    NaiveDate::parse_from_str(tail, "%Y%m%d").ok()
}

fn discover_copy_jobs(
    source_root: &Path,
    output_dir: &Path,
    year: i32,
    since: Option<NaiveDate>,
) -> Result<Vec<CopyJob>> {
    // Каждая месячная папка живёт на сетевой шаре, где обход каталога —
    // сетевой round-trip. Месяцы независимы, поэтому сканируем их параллельно;
    // итоговый порядок задаётся сортировкой по имени целевого файла.
    let month_dirs = month_dirs_by_index(source_root)?
        .into_values()
        .collect::<Vec<_>>();
    let mut jobs = month_dirs
        .par_iter()
        .map(|month_dir| collect_month_jobs(month_dir, output_dir, year, since))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    jobs.sort_by(|left, right| left.target.cmp(&right.target));
    Ok(jobs)
}

/// Задания на копирование из одной месячной папки: каждый подкаталог с именем
/// `дд.мм.гг` за нужный год, где лежат исходные файлы параметров.
fn collect_month_jobs(
    month_dir: &Path,
    output_dir: &Path,
    year: i32,
    since: Option<NaiveDate>,
) -> Result<Vec<CopyJob>> {
    // Дневных папок за месяц три десятка, и каждый их листинг — отдельный
    // сетевой round-trip, поэтому дни обходятся параллельно, как и месяцы.
    let jobs = day_dirs(month_dir, year, since)?
        .par_iter()
        .map(|(day_dir, date)| -> Result<Vec<CopyJob>> {
            Ok(day_source_file(day_dir)?
                .map(|source| CopyJob {
                    target: output_dir.join(target_file_name(*date)),
                    source,
                })
                .into_iter()
                .collect())
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    Ok(jobs)
}

/// Подкаталоги месяца с именем `дд.мм.гг` за нужный год, начиная с даты `since`.
fn day_dirs(
    month_dir: &Path,
    year: i32,
    since: Option<NaiveDate>,
) -> Result<Vec<(PathBuf, NaiveDate)>> {
    let entries = fs::read_dir(month_dir)
        .with_context(|| format!("Не удалось прочитать папку {}", month_dir.display()))?;

    let mut found = Vec::new();
    for entry in entries {
        let entry = entry
            .with_context(|| format!("Не удалось прочитать элемент в {}", month_dir.display()))?;

        // Имя разбирается без обращения к диску, поэтому дата проверяется до
        // типа записи: на сетевой шаре каждый запрос типа — round-trip.
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(date) = parse_day_folder(name, year) else {
            continue;
        };
        // Последняя выгруженная дата перечитывается заново: сводка за неё могла
        // быть неполной на момент прошлого запуска.
        if since.is_some_and(|first| date < first) || !is_dir_entry(&entry) {
            continue;
        }

        found.push((entry.path(), date));
    }

    Ok(found)
}

/// Сводка дневной папки: сначала точечная проверка обычного имени, и только если
/// его нет — перебор папки по маске `Параметры работы скважин*.xls*`.
///
/// Из папки берётся один файл: имя копии стандартное и одно на день, так что
/// второй кандидат всё равно затёр бы первый. При нескольких совпадениях
/// выбирается первое по алфавиту, чтобы результат не зависел от порядка обхода.
fn day_source_file(day_dir: &Path) -> Result<Option<PathBuf>> {
    let primary = day_dir.join(PRIMARY_SOURCE_FILE);
    if primary.is_file() {
        return Ok(Some(primary));
    }

    let entries = fs::read_dir(day_dir)
        .with_context(|| format!("Не удалось прочитать папку {}", day_dir.display()))?;

    let mut found: Option<PathBuf> = None;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("Не удалось прочитать элемент в {}", day_dir.display()))?;

        // Маска проверяется по имени из листинга и ничего не стоит, а тип
        // записи — сетевой запрос. В дневной папке файлов десятки, под маску
        // подходят единицы, поэтому порядок именно такой.
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !matches_source_mask(name) || !is_file_entry(&entry) {
            continue;
        }

        let path = entry.path();
        if found.as_ref().is_none_or(|best| path < *best) {
            found = Some(path);
        }
    }

    Ok(found)
}

fn is_dir_entry(entry: &fs::DirEntry) -> bool {
    entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
}

fn is_file_entry(entry: &fs::DirEntry) -> bool {
    entry
        .file_type()
        .map(|kind| kind.is_file())
        .unwrap_or(false)
}

fn matches_source_mask(file_name: &str) -> bool {
    let path = Path::new(file_name);
    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };

    stem.starts_with(SOURCE_FILE_PREFIX)
        && extension.to_lowercase().starts_with(SOURCE_FILE_EXT_PREFIX)
}

/// Имя копии — стандартное, с датой дневной папки в формате ГГГГММДД.
fn target_file_name(date: NaiveDate) -> String {
    format!(
        "{TARGET_FILE_STEM}_{}.{TARGET_FILE_EXT}",
        date.format("%Y%m%d")
    )
}

fn month_dirs_by_index(source_root: &Path) -> Result<BTreeMap<u32, PathBuf>> {
    let mut found = BTreeMap::new();
    let entries = fs::read_dir(source_root)
        .with_context(|| format!("Не удалось прочитать папку {}", source_root.display()))?;

    for entry in entries {
        let entry = entry
            .with_context(|| format!("Не удалось прочитать элемент в {}", source_root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower_name = name.to_lowercase();
        for (index, month_name) in MONTH_NAMES_LC.iter().enumerate() {
            if lower_name.contains(month_name) {
                found.entry((index + 1) as u32).or_insert(path.clone());
            }
        }
    }

    Ok(found)
}

fn parse_day_folder(folder_name: &str, year: i32) -> Option<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(folder_name, "%d.%m.%y").ok()?;
    if parsed.year() == year {
        Some(parsed)
    } else {
        None
    }
}

fn copy_job(job: &CopyJob) -> Result<PathBuf> {
    fs::copy(&job.source, &job.target).with_context(|| {
        format!(
            "Не удалось скопировать {} -> {}",
            job.source.display(),
            job.target.display()
        )
    })?;
    Ok(job.target.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_day_folder_accepts_matching_year() {
        let parsed = parse_day_folder("07.04.26", 2026);
        assert_eq!(parsed, NaiveDate::from_ymd_opt(2026, 4, 7));
    }

    #[test]
    fn parse_day_folder_rejects_other_year() {
        assert!(parse_day_folder("07.04.25", 2026).is_none());
    }

    #[test]
    fn discover_copy_jobs_collects_existing_daily_files() {
        let root = unique_temp_dir("telemetry_jobs");
        let source_root = root.join("source");
        let output_root = root.join("output");

        let day_dir = source_root.join("8. Август").join("12.08.26");
        fs::create_dir_all(&day_dir).unwrap();
        fs::write(day_dir.join("Параметры работы скважин БНГКМ.xlsm"), b"test").unwrap();

        let jobs = discover_copy_jobs(&source_root, &output_root, 2026, None).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].target,
            output_root.join("Параметры_работы_скважин_БНГКМ_20260812.xlsm")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discover_copy_jobs_merges_months_sorted_by_target() {
        let root = unique_temp_dir("telemetry_multi");
        let source_root = root.join("source");
        let output_root = root.join("output");

        for (month, day) in [("4. Апрель", "07.04.26"), ("3. Март", "03.03.26")] {
            let day_dir = source_root.join(month).join(day);
            fs::create_dir_all(&day_dir).unwrap();
            fs::write(day_dir.join("Параметры работы скважин БНГКМ.xlsm"), b"test").unwrap();
        }

        let jobs = discover_copy_jobs(&source_root, &output_root, 2026, None).unwrap();
        let targets = jobs.iter().map(|job| &job.target).collect::<Vec<_>>();
        assert_eq!(
            targets,
            vec![
                &output_root.join("Параметры_работы_скважин_БНГКМ_20260303.xlsm"),
                &output_root.join("Параметры_работы_скважин_БНГКМ_20260407.xlsm"),
            ]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn primary_file_wins_without_listing_the_rest() {
        let root = unique_temp_dir("telemetry_primary");
        let source_root = root.join("source");
        let output_root = root.join("output");

        let day_dir = source_root.join("8. Август").join("12.08.26");
        fs::create_dir_all(&day_dir).unwrap();
        for name in [
            PRIMARY_SOURCE_FILE,
            "Параметры работы скважин ЯНГКМ.xlsx",
            "Прочий отчёт.xlsx",
        ] {
            fs::write(day_dir.join(name), b"test").unwrap();
        }

        // Обычное имя найдено точечно, поэтому папка не перебирается и соседние
        // файлы под маску уже не рассматриваются.
        let jobs = discover_copy_jobs(&source_root, &output_root, 2026, None).unwrap();
        assert_eq!(
            target_names(&jobs),
            vec!["Параметры_работы_скважин_БНГКМ_20260812.xlsm"]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mask_is_the_fallback_when_the_primary_file_is_missing() {
        let root = unique_temp_dir("telemetry_mask");
        let source_root = root.join("source");
        let output_root = root.join("output");

        let day_dir = source_root.join("8. Август").join("12.08.26");
        fs::create_dir_all(&day_dir).unwrap();
        for name in [
            "Параметры работы скважин ЯНГКМ.xlsx",
            "Параметры работы скважин.xls",
            // Мимо маски: другое начало имени и другое расширение.
            "Режим работы скважин БНГКМ.xlsm",
            "Параметры работы скважин БНГКМ.pdf",
        ] {
            fs::write(day_dir.join(name), b"test").unwrap();
        }

        // Найденное по маске сохраняется под стандартным именем, а не под своим;
        // из двух подходящих берётся первый по алфавиту.
        let jobs = discover_copy_jobs(&source_root, &output_root, 2026, None).unwrap();
        assert_eq!(
            target_names(&jobs),
            vec!["Параметры_работы_скважин_БНГКМ_20260812.xlsm"]
        );
        assert_eq!(
            jobs[0].source.file_name().unwrap(),
            "Параметры работы скважин ЯНГКМ.xlsx"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_name_is_the_same_for_any_source() {
        assert_eq!(
            target_file_name(date(2026, 8, 12)),
            "Параметры_работы_скважин_БНГКМ_20260812.xlsm"
        );
    }

    #[test]
    fn continue_from_last_starts_at_the_newest_exported_date() {
        let root = unique_temp_dir("telemetry_continue");
        let source_root = root.join("source");
        let output_root = root.join("output");

        for (month, day) in [
            ("3. Март", "03.03.26"),
            ("7. Июль", "01.07.26"),
            ("8. Август", "12.08.26"),
        ] {
            let day_dir = source_root.join(month).join(day);
            fs::create_dir_all(&day_dir).unwrap();
            fs::write(day_dir.join(PRIMARY_SOURCE_FILE), b"test").unwrap();
        }
        fs::create_dir_all(&output_root).unwrap();
        fs::write(
            output_root.join("Параметры_работы_скважин_БНГКМ_20260701.xlsm"),
            b"test",
        )
        .unwrap();

        let since = last_exported_date(&output_root).unwrap();
        assert_eq!(since, Some(date(2026, 7, 1)));

        // Июль перечитывается заново, март остаётся позади порога.
        let jobs = discover_copy_jobs(&source_root, &output_root, 2026, since).unwrap();
        assert_eq!(
            target_names(&jobs),
            vec![
                "Параметры_работы_скважин_БНГКМ_20260701.xlsm",
                "Параметры_работы_скважин_БНГКМ_20260812.xlsm",
            ]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn last_exported_date_is_none_for_an_empty_folder() {
        let root = unique_temp_dir("telemetry_empty_out");
        fs::create_dir_all(&root).unwrap();

        assert_eq!(last_exported_date(&root).unwrap(), None);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exported_date_reads_the_suffix_of_a_finished_copy() {
        assert_eq!(
            exported_date("Параметры_работы_скважин_БНГКМ_20260812.xlsm"),
            Some(date(2026, 8, 12))
        );
        assert_eq!(exported_date("Параметры работы скважин БНГКМ.xlsm"), None);
    }

    fn target_names(jobs: &[CopyJob]) -> Vec<&str> {
        jobs.iter()
            .map(|job| job.target.file_name().unwrap().to_str().unwrap())
            .collect()
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("корректная дата")
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{unique}"))
    }
}
