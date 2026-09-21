//! Resolved path bundles used by each processing mode.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PplPaths {
    pub plast_dbf: PathBuf,
    pub wells_xlsx: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SsPaths {
    pub eer1_dbf: PathBuf,
    pub wells_xlsx: PathBuf,
}

#[derive(Debug, Clone)]
pub struct GdiPaths {
    pub stand2_dbf: PathBuf,
    pub ksmest_dbf: PathBuf,
    pub plast_dbf: PathBuf,
    pub stand1_dbf: PathBuf,
    pub wells_xlsx: PathBuf,
}

#[derive(Debug, Clone)]
pub struct VedPaths {
    pub ss_dbf: PathBuf,
    pub ppl_dbf: PathBuf,
    pub wells_xlsx: PathBuf,
    pub templates_dir: PathBuf,
    pub output_dir: PathBuf,
    /// `None` когда папка режимных листов недоступна: колонки
    /// «Технологический режим» тогда не заполняются.
    pub regime_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TelemetryPaths {
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
}

/// Каталоги выгрузок из гидродинамических моделей.
#[derive(Debug, Clone)]
pub struct GdmPaths {
    /// `~/tNavigator_scripts/GDM`: в подпапках по месторождениям лежат CSV.
    pub root: PathBuf,
    /// Справочник привязки скважин к пластам.
    pub link: PathBuf,
    pub result_dir: PathBuf,
}
