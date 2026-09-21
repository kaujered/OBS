//! Геометрия экранов: прямоугольники карточек и кнопок в координатах макета.
//!
//! Числа сняты с исходного интерфейса и масштабируются `LayoutScale`, поэтому
//! правка любого из них двигает элемент на всех разрешениях сразу.

use crate::ui::theme::RectSpec;

pub(crate) const REGULAR_MEST_CARD_RECT: RectSpec = RectSpec::new(349.0, 88.0, 677.0, 192.0);
pub(crate) const REGULAR_MAIN_CARD_RECT: RectSpec = RectSpec::new(350.0, 307.0, 676.0, 645.0);
pub(crate) const REGULAR_LEFT_SECTION_RECT: RectSpec = RectSpec::new(368.0, 345.0, 312.0, 595.0);
pub(crate) const SIMPLE_ACTION_RECT: RectSpec = RectSpec::new(697.0, 344.0, 313.0, 598.0);
pub(crate) const GDI_ACTION_RECT: RectSpec = RectSpec::new(697.0, 344.0, 313.0, 487.0);
pub(crate) const GDI_CHARTS_RECT: RectSpec = RectSpec::new(697.0, 844.0, 313.0, 34.0);
pub(crate) const GDI_DEBUG_RECT: RectSpec = RectSpec::new(697.0, 891.0, 313.0, 34.0);

pub(crate) const BATCH_TOP_MEST_CARD_RECT: RectSpec = RectSpec::new(349.0, 88.0, 676.0, 141.0);
pub(crate) const BATCH_MODULE_CARD_RECT: RectSpec = RectSpec::new(350.0, 248.0, 676.0, 86.0);
pub(crate) const BATCH_MAIN_CARD_RECT: RectSpec = RectSpec::new(350.0, 349.0, 676.0, 603.0);
pub(crate) const BATCH_LEFT_SECTION_RECT: RectSpec = RectSpec::new(368.0, 394.0, 312.0, 547.0);
pub(crate) const BATCH_RIGHT_SECTION_RECT: RectSpec = RectSpec::new(699.0, 394.0, 309.0, 384.0);
pub(crate) const BATCH_ACTION_RECT: RectSpec = RectSpec::new(698.0, 802.0, 312.0, 141.0);

// Карточка месторождений здесь той же высоты, что и на вкладке «Все и сразу»
// (пять чипов укладываются в два ряда), а «Параметры» начинаются на уровне
// карточки модулей и заканчиваются там же, где на остальных вкладках.
pub(crate) const GDM_MEST_CARD_RECT: RectSpec = RectSpec::new(349.0, 88.0, 677.0, 141.0);
pub(crate) const GDM_MAIN_CARD_RECT: RectSpec = RectSpec::new(350.0, 248.0, 676.0, 704.0);
pub(crate) const GDM_LEFT_SECTION_RECT: RectSpec = RectSpec::new(368.0, 286.0, 312.0, 654.0);
pub(crate) const GDM_ACTION_RECT: RectSpec = RectSpec::new(697.0, 285.0, 313.0, 657.0);

pub(crate) const TELEMETRY_MAIN_CARD_RECT: RectSpec = RectSpec::new(350.0, 88.0, 676.0, 864.0);
pub(crate) const TELEMETRY_LEFT_SECTION_RECT: RectSpec = RectSpec::new(368.0, 153.0, 312.0, 783.0);
pub(crate) const TELEMETRY_ACTION_RECT: RectSpec = RectSpec::new(697.0, 152.0, 313.0, 785.0);
pub(crate) const TELEMETRY_CONTINUE_RECT: RectSpec = RectSpec::new(379.0, 340.0, 290.0, 35.0);
