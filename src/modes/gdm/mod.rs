//! Прогнозные таблицы из гидродинамических моделей («из ГДМ»).
//!
//! Источники лежат в `~/tNavigator_scripts/GDM/<месторождение>`: три CSV из
//! калькулятора графиков тНавигатора плюс общий `справочник.xlsx` с привязкой
//! скважин к пластам. Результат — одна книга на месторождение в
//! `~/tNavigator_scripts/GDM/result`.

mod data;
mod source;
mod writer;

pub use data::{DateStep, Request};

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rayon::prelude::*;

use crate::paths;

use self::data::Ind;

/// Месторождение, для которого тНавигатор выгружает `df_one/two/three.csv`.
///
/// Варианты названы кодами каталогов — они же значения ключа `--gdm-mest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum GdmField {
    Bngkm,
    Hgkm,
    Mngkm,
    Ungkm,
    Yangkm,
}

/// Форма отчёта: месторождения различаются набором разрезов по пластам.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Блоки по пластам на листе месторождения и на листах площадей.
    Bngkm,
    /// Блоки по пластам только на листе месторождения.
    Hngkm,
    /// Один пласт: блоков нет, итоги считаются только по площадям.
    OnePlast,
}

impl GdmField {
    pub const ALL: [Self; 5] = [
        Self::Bngkm,
        Self::Hgkm,
        Self::Mngkm,
        Self::Ungkm,
        Self::Yangkm,
    ];

    /// Имя каталога с CSV и листа в справочнике.
    pub fn code(self) -> &'static str {
        match self {
            Self::Bngkm => "bngkm",
            Self::Hgkm => "hngkm",
            Self::Mngkm => "mngkm",
            Self::Ungkm => "ungkm",
            Self::Yangkm => "yangkm",
        }
    }

    pub fn ui_label(self) -> &'static str {
        match self {
            Self::Bngkm => "БНГКМ",
            Self::Hgkm => "ХГКМ",
            Self::Mngkm => "МНГКМ",
            Self::Ungkm => "ЮНГКМ",
            Self::Yangkm => "ЯНГКМ",
        }
    }

    /// Название листа месторождения в итоговой книге.
    pub fn sheet_title(self) -> &'static str {
        match self {
            Self::Bngkm => "Бованенковское",
            Self::Hgkm => "Харасавэйское",
            Self::Mngkm => "Медвежинское",
            Self::Ungkm => "Юбилейное",
            Self::Yangkm => "Ямсовейское",
        }
    }

    /// Площади (ГП) месторождения — в порядке листов отчёта.
    fn groups(self) -> &'static [&'static str] {
        match self {
            Self::Bngkm => &["M1", "M2", "M3"],
            Self::Hgkm => &["M3"],
            Self::Mngkm => &["CH1", "CH3", "CH4", "CH6", "CH8", "CH9"],
            Self::Ungkm => &["U", "UU"],
            Self::Yangkm => &["UKPG-1", "UKPG-2"],
        }
    }

    fn layout(self) -> Layout {
        match self {
            Self::Bngkm => Layout::Bngkm,
            Self::Hgkm => Layout::Hngkm,
            Self::Mngkm | Self::Ungkm | Self::Yangkm => Layout::OnePlast,
        }
    }

    /// Поскважинные листы книги.
    fn dop_sheets(self) -> &'static [&'static str] {
        match self.layout() {
            // Накопленная добыча по скважинам выгружается только там, где
            // отчёт разбит по пластам.
            Layout::OnePlast => &["wbp", "wthp", "wbhp", "wgpr"],
            Layout::Hngkm | Layout::Bngkm => &["wbp", "wthp", "wbhp", "wgpr", "wgpt"],
        }
    }

    /// Дебит на поскважинных листах: исходный скрипт переводит его в
    /// тыс. м3/сут везде, кроме БНГКМ.
    fn well_wgpr_scale(self) -> f64 {
        match self.layout() {
            Layout::Bngkm => 1.0,
            Layout::Hngkm | Layout::OnePlast => 1000.0,
        }
    }

    /// Разрезы, по которым под таблицей скважин печатаются строки-итоги.
    fn dop_inds(self) -> &'static [Ind] {
        match self.layout() {
            Layout::OnePlast => &[Ind::Gp],
            Layout::Hngkm => &[Ind::Gp, Ind::Plast],
            Layout::Bngkm => &[Ind::Gp, Ind::Plast, Ind::PlastGp],
        }
    }
}

pub fn execute(request: &Request) -> Result<Vec<PathBuf>> {
    if request.fields.is_empty() {
        bail!("Выберите хотя бы одно месторождение.");
    }
    if request.start > request.end {
        bail!("Начальная дата выгрузки позже конечной.");
    }

    let paths = paths::resolve_gdm_paths()?;

    // Месторождения независимы, поэтому книги считаются параллельно; внутри
    // каждой параллелятся чтение CSV и агрегаты по пластам и площадям.
    request
        .fields
        .par_iter()
        .map(|field| build_field(*field, &paths, request))
        .collect()
}

fn build_field(field: GdmField, paths: &paths::GdmPaths, request: &Request) -> Result<PathBuf> {
    let dir = paths.root.join(field.code());
    if !dir.is_dir() {
        bail!(
            "Не найдены исходные файлы месторождения {}: {}",
            field.ui_label(),
            dir.display()
        );
    }

    let one_path = dir.join("df_one.csv");
    let two_path = dir.join("df_two.csv");
    let three_path = dir.join("df_three.csv");

    let (one, rest) = rayon::join(
        || source::read_one(&one_path),
        || {
            rayon::join(
                || source::read_two(&two_path),
                || {
                    rayon::join(
                        || source::read_three(&three_path),
                        || source::read_link(&paths.link, field.code()),
                    )
                },
            )
        },
    );
    let (two, (three, link)) = rest;
    let sources = data::FieldSources {
        one: one.with_context(|| format!("Не удалось прочитать {}", one_path.display()))?,
        two: two.with_context(|| format!("Не удалось прочитать {}", two_path.display()))?,
        three: three.with_context(|| format!("Не удалось прочитать {}", three_path.display()))?,
        link: link
            .with_context(|| format!("Не удалось прочитать справочник {}", paths.link.display()))?,
    };

    if data::select_dates_count(&sources.one, request) == 0 {
        bail!(
            "Для месторождения {} нет шагов расчёта в выбранном периоде.",
            field.ui_label()
        );
    }

    let book = data::build(field, &sources, request)?;
    writer::write_workbook(field, &book, &paths.result_dir)
}
