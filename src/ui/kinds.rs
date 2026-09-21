//! Перечисления интерфейса: вкладки главного окна и тема оформления.

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ThemeMode {
    Dark,
    #[default]
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeKind {
    Home,
    Batch,
    Gdi,
    Gdm,
    Ppl,
    Ss,
    Telemetry,
    Ved,
}

impl ModeKind {
    pub const SIDEBAR_ORDER: [Self; 7] = [
        Self::Batch,
        Self::Gdi,
        Self::Ppl,
        Self::Ss,
        Self::Ved,
        Self::Telemetry,
        Self::Gdm,
    ];

    pub fn ui_label(self) -> &'static str {
        match self {
            Self::Home => "Главная",
            Self::Batch => "Все и сразу",
            Self::Gdi => "ГДИ",
            Self::Gdm => "из ГДМ",
            Self::Ppl => "Статика",
            Self::Ss => "Сводка",
            Self::Telemetry => "Телеметрия",
            Self::Ved => "Ведомость",
        }
    }
}
