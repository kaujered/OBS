//! Вкладки главного окна.
//!
//! Каждая вкладка — свой модуль с состоянием формы и отрисовкой. Общее вынесено
//! в соседей: `layout` хранит геометрию, `common` — повторяющиеся элементы.

pub mod batch;
pub mod gdi;
pub mod gdm;
pub mod ppl;
pub mod ss;
pub mod telemetry;
pub mod ved;

pub(crate) mod common;
pub(crate) mod layout;

pub(crate) use common::{
    draw_outer_panel, primary_action_label, primary_action_visuals, render_batch_mest_card,
    render_batch_module_card, render_regular_mest_card,
};
pub(crate) use layout::*;
