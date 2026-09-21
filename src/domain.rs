//! Справочник месторождений: коды, подписи в интерфейсе и в именах выгрузок.
//!
//! Домен, общий для интерфейса и расчётных модулей. Правки здесь меняют и
//! подписи кнопок, и названия файлов, поэтому таблица одна на всех.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mest {
    Mgpu = 1,
    YungkmSenoman = 2,
    Yangkm = 3,
    Bngkm = 4,
    Hgkm = 5,
    MgpuNyda = 7,
    YungkmAptAlb = 8,
}

impl Mest {
    pub const ALL: [Self; 7] = [
        Self::Mgpu,
        Self::YungkmSenoman,
        Self::Yangkm,
        Self::Bngkm,
        Self::Hgkm,
        Self::MgpuNyda,
        Self::YungkmAptAlb,
    ];

    pub const DISPLAY_ROWS: [&'static [Self]; 3] = [
        &[Self::Mgpu, Self::YungkmSenoman, Self::Bngkm],
        &[Self::MgpuNyda, Self::YungkmAptAlb, Self::Hgkm],
        &[Self::Yangkm],
    ];

    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn ui_label(self) -> &'static str {
        match self {
            Self::Mgpu => "МГПУ",
            Self::YungkmSenoman => "ЮНГКМ_сеноман",
            Self::Yangkm => "ЯНГКМ",
            Self::Bngkm => "БНГКМ",
            Self::Hgkm => "ХГКМ",
            Self::MgpuNyda => "МГПУ_Ныда",
            Self::YungkmAptAlb => "ЮНГКМ_апт-альб",
        }
    }

    pub fn output_label(self) -> &'static str {
        match self {
            Self::Mgpu => "Медвежье",
            Self::YungkmSenoman => "Юбилейное_сеноман",
            Self::Yangkm => "Ямсовейское",
            Self::Bngkm => "Бованенковское",
            Self::Hgkm => "Харасавэйское",
            Self::MgpuNyda => "Ныда",
            Self::YungkmAptAlb => "Юбилейное_апт-альб",
        }
    }

    pub fn wells_sheet_name(self) -> &'static str {
        self.ui_label()
    }

    /// Fields whose data is folded into THIS field's выгрузка.
    /// МГПУ absorbs Ныда (МГПУ_Ныда) so a single МГПУ run carries both fields;
    /// every other field stands alone. Used by GDI/Статика/Сводка to merge the
    /// rows onto one sheet, and by Ведомость to also emit the separate Ныда file.
    pub fn merge_sources(self) -> &'static [Mest] {
        match self {
            Self::Mgpu => &[Self::Mgpu, Self::MgpuNyda],
            Self::YungkmSenoman => &[Self::YungkmSenoman],
            Self::Yangkm => &[Self::Yangkm],
            Self::Bngkm => &[Self::Bngkm],
            Self::Hgkm => &[Self::Hgkm],
            Self::MgpuNyda => &[Self::MgpuNyda],
            Self::YungkmAptAlb => &[Self::YungkmAptAlb],
        }
    }

    pub fn is_bngkm(self) -> bool {
        matches!(self, Self::Bngkm)
    }

    pub fn is_hgkm(self) -> bool {
        matches!(self, Self::Hgkm)
    }
}
