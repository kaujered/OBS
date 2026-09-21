//! Shared CLI options for the GUI binary.
//!
//! This module only decides how the application should start.
//! It does not execute business logic directly.

use chrono::{Datelike, Local, NaiveDate};
use clap::{Parser, ValueEnum};

use crate::domain::Mest;
use crate::modes::gdm::{DateStep, GdmField};
use crate::ui::kinds::{ModeKind, ThemeMode};

/// Заголовок группы ключей «из ГДМ» в выводе `--help`.
const GDM_HEADING: &str = "Выгрузка «из ГДМ»";

const GDM_HELP: &str = "\
Выгрузка «из ГДМ» — прогнозные таблицы по расчётам тНавигатора.

  Исходные файлы:  ~/tNavigator_scripts/GDM/<месторождение>/df_one.csv,
                   df_two.csv, df_three.csv и общий справочник.xlsx
  Результат:       ~/tNavigator_scripts/GDM/result/qmain_<месторождение>_прогноз_<дата>.xlsx

  --gdm-step повторяет кнопки вкладки: all — «Все даты», year — «За год»,
  quarter — «За квартал», month — «За месяц».
  Обе границы периода включительны; min и max берут первый и последний шаг
  расчёта, без ключей период не ограничен.
  --run gdm открывает вкладку, ничего не запуская.

Примеры:
  one_big_script_rs_all_in_one --gdm --gdm-mest bngkm --gdm-step year
  one_big_script_rs_all_in_one --gdm --gdm-mest hgkm,mngkm \\
      --gdm-start 01012020 --gdm-end 01012040 --gdm-step quarter
  one_big_script_rs_all_in_one --gdm --gdm-start min --gdm-end max --gdm-step all
  one_big_script_rs_all_in_one --gdm            # все месторождения, помесячно";

#[derive(Debug, Clone, Parser)]
#[command(
    author,
    version,
    about = "Compact native Rust desktop for GDI, PPL, SS, Telemetry, Ved and ГДМ.",
    after_help = GDM_HELP
)]
pub struct Cli {
    /// Open a specific module immediately instead of the default batch screen.
    #[arg(long, value_enum)]
    pub run: Option<ModeCli>,
    /// UI theme for the desktop shell.
    #[arg(long, value_enum, default_value_t = ThemeMode::Light)]
    pub theme: ThemeMode,

    /// Run GDI headlessly without GUI and exit.
    /// Combine with --bngkm / --hgkm to select fields.
    #[arg(long)]
    pub gdi: bool,

    /// Run Суточная сводка (SS) headlessly without GUI and exit.
    #[arg(long)]
    pub ss: bool,

    /// Run Статика (PPL) headlessly without GUI and exit.
    #[arg(long)]
    pub ppl: bool,

    /// Run Ведомость (VED) headlessly without GUI and exit.
    /// Combine with mest flags, --date, --test, --debug, --no-correct.
    #[arg(long)]
    pub ved: bool,

    /// Include БНГКМ (Бованенковское) in headless GDI export
    #[arg(long)]
    pub bngkm: bool,

    /// Include ХГКМ (Харасавэйское) in headless GDI export
    #[arg(long)]
    pub hgkm: bool,

    /// Include МГПУ in headless GDI export
    #[arg(long)]
    pub mgpu: bool,

    /// Include МГПУ_Ныда in headless GDI export
    #[arg(long)]
    pub nyda: bool,

    /// Include ЮНГКМ_сеноман in headless GDI export
    #[arg(long)]
    pub yungkm: bool,

    /// Include ЮНГКМ_апт-альб in headless GDI export
    #[arg(long, name = "apt-alb")]
    pub apt_alb: bool,

    /// Include ЯНГКМ in headless GDI export
    #[arg(long)]
    pub yangkm: bool,

    /// Use XLSX/КОЦ source for БНГКМ (instead of DBF). Requires --bngkm.
    #[arg(long)]
    pub xlsx: bool,

    /// Date filter in DDMMYYYY format (e.g. --date 01012020 = 01 Jan 2020)
    #[arg(long)]
    pub date: Option<String>,

    /// Export only the last study per well (headless GDI)
    #[arg(long = "only-last")]
    pub only_last: bool,

    /// Include rejected studies / отбракованные (headless GDI)
    #[arg(long)]
    pub brak: bool,

    /// Use TEST folder (headless GDI)
    #[arg(long)]
    pub test: bool,

    /// Include charts sheet "ГРАФИКИ" in GDI output
    #[arg(long)]
    pub graph: bool,

    /// Debug mode for headless Ведомость (keeps intermediate files, skips replication)
    #[arg(long)]
    pub debug: bool,

    /// Disable "Корректировать рабочие параметры" in headless Ведомость (on by default)
    #[arg(long = "no-correct")]
    pub no_correct: bool,

    /// Add FILTRED_GDI sheet with rows on or after this date (DDMMYYYY format)
    #[arg(long = "filtr_date")]
    pub filtr_date: Option<String>,

    /// Выгрузить прогнозные таблицы «из ГДМ» и закрыть окно по завершении
    #[arg(long, help_heading = GDM_HEADING)]
    pub gdm: bool,

    /// Месторождения через запятую; без ключа выгружаются все пять
    #[arg(
        long = "gdm-mest",
        value_enum,
        value_delimiter = ',',
        value_name = "СПИСОК",
        help_heading = GDM_HEADING
    )]
    pub gdm_mest: Vec<GdmField>,

    /// Начало периода включительно: ДДММГГГГ либо min — с первого шага расчёта
    #[arg(
        long = "gdm-start",
        value_parser = parse_gdm_bound,
        value_name = "ДДММГГГГ|min",
        help_heading = GDM_HEADING
    )]
    pub gdm_start: Option<NaiveDate>,

    /// Конец периода включительно: ДДММГГГГ либо max — до последнего шага расчёта
    #[arg(
        long = "gdm-end",
        value_parser = parse_gdm_bound,
        value_name = "ДДММГГГГ|max",
        help_heading = GDM_HEADING
    )]
    pub gdm_end: Option<NaiveDate>,

    /// Шаг выгрузки: все даты / за год / за квартал / за месяц
    #[arg(
        long = "gdm-step",
        value_enum,
        default_value_t = DateStep::Month,
        value_name = "ШАГ",
        help_heading = GDM_HEADING
    )]
    pub gdm_step: DateStep,
}

/// Граница периода «из ГДМ»: `min`/`max` оставляют её открытой, иначе дата
/// в том же формате ДДММГГГГ, что и у остальных режимов.
fn parse_gdm_bound(text: &str) -> Result<NaiveDate, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "min" => Ok(NaiveDate::MIN),
        "max" => Ok(NaiveDate::MAX),
        _ => NaiveDate::parse_from_str(text.trim(), "%d%m%Y").map_err(|_| {
            format!("ожидается дата в формате ДДММГГГГ (например 01012020) либо min/max, получено «{text}»")
        }),
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ModeCli {
    Batch,
    Gdi,
    Gdm,
    Ppl,
    Ss,
    Telemetry,
    Ved,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }

    /// Convert raw CLI mode into the screen enum used by the GUI.
    /// Autorun flags (--gdi, --ss, --ppl, --ved) take priority over --run.
    pub fn initial_mode(&self) -> ModeKind {
        if self.gdi {
            return ModeKind::Gdi;
        }
        if self.ss {
            return ModeKind::Ss;
        }
        if self.ppl {
            return ModeKind::Ppl;
        }
        if self.ved {
            return ModeKind::Ved;
        }
        if self.gdm {
            return ModeKind::Gdm;
        }
        match self.run {
            Some(ModeCli::Batch) => ModeKind::Batch,
            Some(ModeCli::Gdi) => ModeKind::Gdi,
            Some(ModeCli::Gdm) => ModeKind::Gdm,
            Some(ModeCli::Ppl) => ModeKind::Ppl,
            Some(ModeCli::Ss) => ModeKind::Ss,
            Some(ModeCli::Telemetry) => ModeKind::Telemetry,
            Some(ModeCli::Ved) => ModeKind::Ved,
            None => ModeKind::Batch,
        }
    }

    /// Returns true if any headless autorun flag is set.
    pub fn is_autorun(&self) -> bool {
        self.gdi || self.ss || self.ppl || self.ved || self.gdm
    }

    /// Месторождения для выгрузки «из ГДМ»; без ключа — все пять.
    pub fn parsed_gdm_fields(&self) -> Vec<GdmField> {
        if self.gdm_mest.is_empty() {
            return GdmField::ALL.to_vec();
        }
        let mut fields = self.gdm_mest.clone();
        fields.sort_unstable();
        fields.dedup();
        fields
    }

    /// Границы периода «из ГДМ». Открытая граница (`min`/`max`, а также
    /// пропущенный ключ) отдаётся предельной датой, чтобы не отсечь ни одного
    /// шага расчёта.
    pub fn parsed_gdm_period(&self) -> (NaiveDate, NaiveDate) {
        (
            self.gdm_start.unwrap_or(NaiveDate::MIN),
            self.gdm_end.unwrap_or(NaiveDate::MAX),
        )
    }

    /// Parse --date (DDMMYYYY) into (year, month, day).
    /// Defaults to (current_year, 1, 1) if not provided or unparseable.
    pub fn parsed_date(&self) -> (i32, u32, u32) {
        let Some(s) = &self.date else {
            return (Local::now().year(), 1, 1);
        };
        if s.len() == 8 {
            if let (Ok(day), Ok(month), Ok(year)) = (
                s[0..2].parse::<u32>(),
                s[2..4].parse::<u32>(),
                s[4..8].parse::<i32>(),
            ) {
                return (year, month, day);
            }
        }
        (Local::now().year(), 1, 1)
    }

    /// Parse --filtr_date (DDMMYYYY) into a YYYYMMDD i64 key, or None if absent/unparseable.
    pub fn parsed_filtr_date(&self) -> Option<i64> {
        let s = self.filtr_date.as_deref()?;
        if s.len() == 8 {
            let day = s[0..2].parse::<i64>().ok()?;
            let month = s[2..4].parse::<i64>().ok()?;
            let year = s[4..8].parse::<i64>().ok()?;
            Some(year * 10_000 + month * 100 + day)
        } else {
            None
        }
    }

    /// Collect the list of mests from individual flags.
    /// Returns all mests if no specific flag is set.
    pub fn parsed_mests(&self) -> Vec<Mest> {
        let mut mests = Vec::new();
        if self.bngkm {
            mests.push(Mest::Bngkm);
        }
        if self.hgkm {
            mests.push(Mest::Hgkm);
        }
        if self.mgpu {
            mests.push(Mest::Mgpu);
        }
        if self.nyda {
            mests.push(Mest::MgpuNyda);
        }
        if self.yungkm {
            mests.push(Mest::YungkmSenoman);
        }
        if self.apt_alb {
            mests.push(Mest::YungkmAptAlb);
        }
        if self.yangkm {
            mests.push(Mest::Yangkm);
        }
        if mests.is_empty() {
            mests = Mest::ALL.to_vec();
        }
        mests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        let mut full = vec!["one_big_script"];
        full.extend_from_slice(args);
        Cli::parse_from(full)
    }

    #[test]
    fn gdm_bound_accepts_date_and_keywords() {
        assert_eq!(
            parse(&["--gdm-start", "01012020"]).gdm_start,
            NaiveDate::from_ymd_opt(2020, 1, 1)
        );
        assert_eq!(
            parse(&["--gdm-start", "MIN"]).gdm_start,
            Some(NaiveDate::MIN)
        );
        assert_eq!(parse(&["--gdm-end", "max"]).gdm_end, Some(NaiveDate::MAX));
        assert!(parse_gdm_bound("2020-01-01").is_err());
    }

    #[test]
    fn gdm_period_is_open_without_keys() {
        let (start, end) = parse(&["--gdm"]).parsed_gdm_period();
        assert_eq!((start, end), (NaiveDate::MIN, NaiveDate::MAX));
    }

    #[test]
    fn gdm_fields_default_to_every_field() {
        assert_eq!(
            parse(&["--gdm"]).parsed_gdm_fields(),
            GdmField::ALL.to_vec()
        );
        assert_eq!(
            parse(&["--gdm", "--gdm-mest", "hgkm,bngkm,hgkm"]).parsed_gdm_fields(),
            vec![GdmField::Bngkm, GdmField::Hgkm]
        );
    }

    #[test]
    fn gdm_flag_opens_and_autoruns_its_screen() {
        let cli = parse(&["--gdm"]);
        assert_eq!(cli.initial_mode(), ModeKind::Gdm);
        assert!(cli.is_autorun());
        // --run только открывает вкладку, ничего не запуская.
        let cli = parse(&["--run", "gdm"]);
        assert_eq!(cli.initial_mode(), ModeKind::Gdm);
        assert!(!cli.is_autorun());
    }
}
