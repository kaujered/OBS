use chrono::{Datelike, Local};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use egui::{Align2, Stroke};

use crate::cli::Cli;
use crate::domain::Mest;
use crate::modes::ss;
use crate::ui::app::{
    TaskState, default_selected_mests, default_year, finish_with_paths, month_options,
    spawn_result_task, year_options,
};
use crate::ui::components::{
    ButtonVisuals, paint_text, put_button, put_button_rich, put_scrollable_combo_box,
    show_inner_section,
};
use crate::ui::theme::{LayoutScale, Palette, RectSpec};

use super::{
    REGULAR_LEFT_SECTION_RECT, REGULAR_MAIN_CARD_RECT, SIMPLE_ACTION_RECT, draw_outer_panel,
    primary_action_label, primary_action_visuals, render_regular_mest_card,
};

pub(crate) struct SsScreen {
    selected: BTreeSet<Mest>,
    year: i32,
    month: u32,
    test_mode: bool,
    debug_mode: bool,
    only_last: bool,
    task: TaskState<Vec<PathBuf>>,
    /// When true, the task is started automatically on the first render frame.
    pending_start: bool,
}

impl Default for SsScreen {
    fn default() -> Self {
        Self {
            selected: default_selected_mests(),
            year: default_year(),
            month: Local::now().month(),
            test_mode: false,
            debug_mode: false,
            only_last: true,
            task: TaskState::default(),
            pending_start: false,
        }
    }
}

impl SsScreen {
    /// Build a pre-filled screen from CLI flags and schedule an automatic run.
    pub(crate) fn from_cli(cli: &Cli) -> Self {
        let (year, month, _day) = cli.parsed_date();
        let selected = cli.parsed_mests().into_iter().collect();
        Self {
            selected,
            year,
            month,
            test_mode: cli.test,
            debug_mode: false,
            only_last: cli.only_last,
            task: TaskState::default(),
            pending_start: true,
        }
    }

    pub(crate) fn render(&mut self, ui: &mut egui::Ui, scale: &LayoutScale, palette: Palette) {
        self.poll();

        // Auto-start: triggered on first render when launched from CLI.
        if self.pending_start && !self.task.running && !self.selected.is_empty() {
            self.pending_start = false;
            self.start_task();
        }

        render_regular_mest_card(ui, scale, palette, &mut self.selected);
        draw_outer_panel(ui, scale, palette, REGULAR_MAIN_CARD_RECT, "Параметры");
        show_inner_section(
            ui,
            scale.rect(REGULAR_LEFT_SECTION_RECT),
            scale,
            palette,
            |_| {},
        );

        paint_text(
            ui,
            scale,
            524.0,
            371.0,
            Align2::CENTER_CENTER,
            "Для Сводки",
            15.0,
            palette.text,
        );
        ui.painter().line_segment(
            [scale.point(402.0, 392.0), scale.point(646.0, 392.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        paint_text(
            ui,
            scale,
            524.0,
            413.0,
            Align2::CENTER_CENTER,
            "Дата",
            14.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            510.0,
            450.0,
            Align2::RIGHT_CENTER,
            "Год",
            14.0,
            palette.text,
        );
        let year_choices = year_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(520.0, 434.0, 100.0, 28.0)),
            scale,
            palette,
            "ss_year_combo",
            &mut self.year,
            year_choices,
            |year| year.to_string(),
        );
        paint_text(
            ui,
            scale,
            510.0,
            492.0,
            Align2::RIGHT_CENTER,
            "Месяц",
            14.0,
            palette.text,
        );
        let month_choices = month_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(520.0, 478.0, 100.0, 28.0)),
            scale,
            palette,
            "ss_month_combo",
            &mut self.month,
            month_choices,
            |month| format!("{month:02}"),
        );

        ui.painter().line_segment(
            [scale.point(402.0, 522.0), scale.point(646.0, 522.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        paint_text(
            ui,
            scale,
            524.0,
            545.0,
            Align2::CENTER_CENTER,
            "Опции",
            14.0,
            palette.text,
        );

        if put_button(
            ui,
            scale.rect(RectSpec::new(379.0, 567.0, 290.0, 35.0)),
            scale,
            "Только последняя сводка",
            if self.only_last {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.only_last = !self.only_last;
        }

        if put_button(
            ui,
            scale.rect(RectSpec::new(377.0, 617.0, 290.0, 35.0)),
            scale,
            "Использовать папку TEST",
            if self.test_mode {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.test_mode = !self.test_mode;
        }

        if put_button(
            ui,
            scale.rect(RectSpec::new(377.0, 665.0, 290.0, 35.0)),
            scale,
            "Debug",
            if self.debug_mode {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.debug_mode = !self.debug_mode;
        }

        let has_selected_mest = !self.selected.is_empty();
        let can_run = !self.task.running && has_selected_mest;
        let response = put_button_rich(
            ui,
            scale.rect(SIMPLE_ACTION_RECT),
            scale,
            primary_action_label(has_selected_mest),
            primary_action_visuals(palette, has_selected_mest, can_run),
            18.0,
            20.0,
        );

        if can_run && response.clicked() {
            self.start_task();
        }
    }

    /// Start the SS export task.
    fn start_task(&mut self) {
        let request = ss::Request {
            mests: self.selected.iter().copied().collect(),
            year: self.year,
            month: self.month,
            test_mode: self.test_mode,
            debug_mode: self.debug_mode,
            only_last: self.only_last,
        };
        let (tx, rx) = mpsc::channel();
        self.task.start(rx, "Запущено формирование сводки...");
        spawn_result_task(tx, "Сводка", move || ss::execute(&request));
    }

    fn poll(&mut self) {
        self.task.update_with(finish_with_paths, || {
            "Фоновая задача сводки была прервана.".to_string()
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
