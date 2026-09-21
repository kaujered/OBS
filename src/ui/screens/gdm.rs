use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use chrono::NaiveDate;
use egui::{Align2, Stroke};
use once_cell::sync::Lazy;

use crate::cli::Cli;
use crate::modes::gdm::{self, DateStep, GdmField};
use crate::ui::app::{TaskState, day_options, finish_with_paths, month_options, spawn_result_task};
use crate::ui::components::{
    ButtonVisuals, paint_text, put_button, put_button_rich, put_scrollable_combo_box, show_card,
    show_inner_section,
};
use crate::ui::theme::{CARD_RADIUS, LayoutScale, Palette, RectSpec, SMALL_BUTTON_RADIUS};

use super::{
    GDM_ACTION_RECT, GDM_LEFT_SECTION_RECT, GDM_MAIN_CARD_RECT, GDM_MEST_CARD_RECT,
    draw_outer_panel, primary_action_label, primary_action_visuals,
};

/// Модели считают прогноз на десятилетия вперёд, поэтому список лет шире, чем
/// у отчётов по фактическим данным.
const FIRST_YEAR: i32 = 1971;
const LAST_YEAR: i32 = 2100;

static GDM_YEAR_OPTIONS: Lazy<Vec<i32>> = Lazy::new(|| (FIRST_YEAR..=LAST_YEAR).rev().collect());

const STEP_BUTTONS: [(DateStep, f32); 4] = [
    (DateStep::All, 508.0),
    (DateStep::Year, 551.0),
    (DateStep::Quarter, 594.0),
    (DateStep::Month, 637.0),
];

/// Календарная дата, разложенная по трём выпадающим спискам.
#[derive(Clone, Copy)]
struct DateParts {
    year: i32,
    month: u32,
    day: u32,
}

impl DateParts {
    /// Раскладывает дату по спискам; выходящие за них годы подтягиваются к
    /// краю диапазона, чтобы комбобокс всегда показывал существующий пункт.
    fn from_date(date: NaiveDate) -> Self {
        use chrono::Datelike;
        Self {
            year: date.year().clamp(FIRST_YEAR, LAST_YEAR),
            month: date.month(),
            day: date.day(),
        }
    }

    /// Собирает дату, подтягивая день к концу месяца, — так выбор 31-го числа
    /// не ломает февраль.
    fn to_date(self) -> NaiveDate {
        for day in (1..=self.day).rev() {
            if let Some(date) = NaiveDate::from_ymd_opt(self.year, self.month, day) {
                return date;
            }
        }
        NaiveDate::from_ymd_opt(self.year, self.month, 1).unwrap_or_default()
    }
}

pub(crate) struct GdmScreen {
    selected: BTreeSet<GdmField>,
    start: DateParts,
    end: DateParts,
    step: DateStep,
    task: TaskState<Vec<PathBuf>>,
    /// Период из командной строки. Открытую границу (`--gdm-start min`,
    /// `--gdm-end max`) выпадающими списками не выразить, поэтому она хранится
    /// отдельно и расходуется автозапуском; кнопка в интерфейсе всегда берёт
    /// даты из списков.
    cli_period: Option<(NaiveDate, NaiveDate)>,
    /// Автозапуск на первом кадре — когда приложение открыто ключом `--gdm`.
    pending_start: bool,
}

impl Default for GdmScreen {
    fn default() -> Self {
        Self {
            selected: BTreeSet::new(),
            start: DateParts {
                year: FIRST_YEAR,
                month: 1,
                day: 1,
            },
            end: DateParts {
                year: LAST_YEAR,
                month: 1,
                day: 1,
            },
            step: DateStep::Month,
            task: TaskState::default(),
            cli_period: None,
            pending_start: false,
        }
    }
}

impl GdmScreen {
    /// Экран, заполненный ключами командной строки, с отложенным автозапуском.
    pub(crate) fn from_cli(cli: &Cli) -> Self {
        let (start, end) = cli.parsed_gdm_period();
        Self {
            selected: cli.parsed_gdm_fields().into_iter().collect(),
            start: DateParts::from_date(start),
            end: DateParts::from_date(end),
            step: cli.gdm_step,
            task: TaskState::default(),
            cli_period: Some((start, end)),
            pending_start: true,
        }
    }

    pub(crate) fn render(&mut self, ui: &mut egui::Ui, scale: &LayoutScale, palette: Palette) {
        self.poll();

        // Автозапуск: срабатывает на первом кадре при запуске ключом --gdm.
        if self.pending_start && !self.task.running && !self.selected.is_empty() {
            self.pending_start = false;
            let period = self.cli_period.take();
            self.start_task(period);
        }

        self.render_field_card(ui, scale, palette);
        draw_outer_panel(ui, scale, palette, GDM_MAIN_CARD_RECT, "Параметры");
        show_inner_section(
            ui,
            scale.rect(GDM_LEFT_SECTION_RECT),
            scale,
            palette,
            |_| {},
        );

        paint_text(
            ui,
            scale,
            524.0,
            312.0,
            Align2::CENTER_CENTER,
            "Прогноз из ГДМ",
            15.0,
            palette.text,
        );
        ui.painter().line_segment(
            [scale.point(402.0, 333.0), scale.point(646.0, 333.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        paint_text(
            ui,
            scale,
            524.0,
            354.0,
            Align2::CENTER_CENTER,
            "Период выгрузки",
            14.0,
            palette.text,
        );
        render_date_row(
            ui,
            scale,
            palette,
            "Начало",
            391.0,
            "gdm_start",
            &mut self.start,
        );
        render_date_row(
            ui,
            scale,
            palette,
            "Окончание",
            433.0,
            "gdm_end",
            &mut self.end,
        );

        ui.painter().line_segment(
            [scale.point(402.0, 463.0), scale.point(646.0, 463.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );
        paint_text(
            ui,
            scale,
            524.0,
            486.0,
            Align2::CENTER_CENTER,
            "Шаг выгрузки",
            14.0,
            palette.text,
        );
        for (step, y) in STEP_BUTTONS {
            if put_button(
                ui,
                scale.rect(RectSpec::new(379.0, y, 290.0, 35.0)),
                scale,
                step.ui_label(),
                if self.step == step {
                    ButtonVisuals::active(palette)
                } else {
                    ButtonVisuals::neutral(palette)
                },
                13.0,
                14.0,
            )
            .clicked()
            {
                self.step = step;
            }
        }

        let has_selected_mest = !self.selected.is_empty();
        let can_run = !self.task.running && has_selected_mest;
        let response = put_button_rich(
            ui,
            scale.rect(GDM_ACTION_RECT),
            scale,
            primary_action_label(has_selected_mest),
            primary_action_visuals(palette, has_selected_mest, can_run),
            18.0,
            20.0,
        );

        if can_run && response.clicked() {
            self.start_task(None);
        }
    }

    fn render_field_card(&mut self, ui: &mut egui::Ui, scale: &LayoutScale, palette: Palette) {
        const CHIP_RECTS: [(GdmField, RectSpec); 5] = [
            (GdmField::Bngkm, RectSpec::new(369.0, 128.0, 205.0, 37.0)),
            (GdmField::Hgkm, RectSpec::new(588.0, 128.0, 205.0, 37.0)),
            (GdmField::Mngkm, RectSpec::new(807.0, 128.0, 205.0, 37.0)),
            (GdmField::Ungkm, RectSpec::new(478.0, 179.0, 205.0, 37.0)),
            (GdmField::Yangkm, RectSpec::new(697.0, 179.0, 205.0, 37.0)),
        ];

        show_card(
            ui,
            scale.rect(GDM_MEST_CARD_RECT),
            scale,
            palette,
            palette.card,
            CARD_RADIUS,
            14.0,
            true,
            |_| {},
        );
        paint_text(
            ui,
            scale,
            405.0,
            107.0,
            Align2::LEFT_CENTER,
            "Месторождения",
            16.0,
            palette.text,
        );

        if put_button(
            ui,
            scale.rect(RectSpec::new(588.0, 96.0, 97.0, 22.0)),
            scale,
            "Выбрать все",
            ButtonVisuals::neutral(palette),
            12.0,
            SMALL_BUTTON_RADIUS,
        )
        .clicked()
        {
            self.selected.extend(GdmField::ALL);
        }

        if put_button(
            ui,
            scale.rect(RectSpec::new(694.0, 96.0, 97.0, 22.0)),
            scale,
            "Очистить",
            ButtonVisuals::neutral(palette),
            12.0,
            SMALL_BUTTON_RADIUS,
        )
        .clicked()
        {
            self.selected.clear();
        }

        for (field, rect) in CHIP_RECTS {
            let is_active = self.selected.contains(&field);
            if put_button(
                ui,
                scale.rect(rect),
                scale,
                field.ui_label(),
                if is_active {
                    ButtonVisuals::active(palette)
                } else {
                    ButtonVisuals::neutral(palette)
                },
                14.0,
                SMALL_BUTTON_RADIUS,
            )
            .clicked()
            {
                if is_active {
                    self.selected.remove(&field);
                } else {
                    self.selected.insert(field);
                }
            }
        }
    }

    fn start_task(&mut self, period: Option<(NaiveDate, NaiveDate)>) {
        let (start, end) = period.unwrap_or_else(|| (self.start.to_date(), self.end.to_date()));
        let request = gdm::Request {
            fields: self.selected.iter().copied().collect(),
            start,
            end,
            step: self.step,
        };
        let (tx, rx) = mpsc::channel();
        self.task
            .start(rx, "Запущено формирование прогнозных таблиц...");
        spawn_result_task(tx, "из ГДМ", move || gdm::execute(&request));
    }

    fn poll(&mut self) {
        self.task.update_with(finish_with_paths, || {
            "Фоновая задача «из ГДМ» была прервана.".to_string()
        });
    }

    pub(crate) fn status_and_paths(&self) -> (&str, &[PathBuf]) {
        self.task.status_and_paths()
    }

    pub(crate) fn is_running(&self) -> bool {
        self.task.running
    }

    pub(crate) fn running_for(&self) -> Option<Duration> {
        self.task.running_for()
    }
}

fn render_date_row(
    ui: &mut egui::Ui,
    scale: &LayoutScale,
    palette: Palette,
    label: &str,
    y: f32,
    id_prefix: &str,
    parts: &mut DateParts,
) {
    paint_text(
        ui,
        scale,
        382.0,
        y,
        Align2::LEFT_CENTER,
        label,
        14.0,
        palette.text,
    );
    put_scrollable_combo_box(
        ui,
        scale.rect(RectSpec::new(462.0, y - 14.0, 62.0, 28.0)),
        scale,
        palette,
        format!("{id_prefix}_day"),
        &mut parts.day,
        day_options(),
        |day| format!("{day:02}"),
    );
    put_scrollable_combo_box(
        ui,
        scale.rect(RectSpec::new(530.0, y - 14.0, 62.0, 28.0)),
        scale,
        palette,
        format!("{id_prefix}_month"),
        &mut parts.month,
        month_options(),
        |month| format!("{month:02}"),
    );
    put_scrollable_combo_box(
        ui,
        scale.rect(RectSpec::new(598.0, y - 14.0, 78.0, 28.0)),
        scale,
        palette,
        format!("{id_prefix}_year"),
        &mut parts.year,
        GDM_YEAR_OPTIONS.as_slice(),
        |year| year.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_parts_clamp_day_to_month_end() {
        let parts = DateParts {
            year: 2025,
            month: 2,
            day: 31,
        };
        assert_eq!(
            parts.to_date(),
            NaiveDate::from_ymd_opt(2025, 2, 28).expect("конец февраля")
        );
    }
}
