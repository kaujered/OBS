use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use egui::{Align2, Stroke};

use crate::modes::telemetry;
use crate::ui::app::{TaskState, default_year, finish_with_paths, spawn_result_task, year_options};
use crate::ui::components::{
    ButtonVisuals, paint_text, put_button, put_scrollable_combo_box, show_inner_section,
};
use crate::ui::theme::{LayoutScale, Palette, RectSpec};

use super::{
    TELEMETRY_ACTION_RECT, TELEMETRY_CONTINUE_RECT, TELEMETRY_LEFT_SECTION_RECT,
    TELEMETRY_MAIN_CARD_RECT, draw_outer_panel,
};

pub(crate) struct TelemetryScreen {
    year: i32,
    /// Продолжать с последней выгруженной сводки. По умолчанию включено: обычно
    /// нужно добрать свежие дни, а не перекладывать год целиком.
    continue_from_last: bool,
    task: TaskState<Vec<PathBuf>>,
}

impl Default for TelemetryScreen {
    fn default() -> Self {
        Self {
            year: 0,
            continue_from_last: true,
            task: TaskState::default(),
        }
    }
}

impl TelemetryScreen {
    pub(crate) fn render(&mut self, ui: &mut egui::Ui, scale: &LayoutScale, palette: Palette) {
        if self.year == 0 {
            self.year = default_year();
        }
        self.poll();

        draw_outer_panel(ui, scale, palette, TELEMETRY_MAIN_CARD_RECT, "Параметры");
        show_inner_section(
            ui,
            scale.rect(TELEMETRY_LEFT_SECTION_RECT),
            scale,
            palette,
            |_| {},
        );

        // TODO: the Figma export still says "Для Сводки" on the telemetry screen.
        // Keeping the visible text from the design while binding the controls to telemetry logic.
        paint_text(
            ui,
            scale,
            524.0,
            187.0,
            Align2::CENTER_CENTER,
            "Для Сводки",
            15.0,
            palette.text,
        );
        ui.painter().line_segment(
            [scale.point(402.0, 209.0), scale.point(646.0, 209.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        paint_text(
            ui,
            scale,
            524.0,
            230.0,
            Align2::CENTER_CENTER,
            "Дата",
            14.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            510.0,
            272.0,
            Align2::RIGHT_CENTER,
            "Год",
            14.0,
            palette.text,
        );
        let year_choices = year_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(520.0, 256.0, 100.0, 28.0)),
            scale,
            palette,
            "telemetry_year_combo",
            &mut self.year,
            year_choices,
            |year| year.to_string(),
        );

        ui.painter().line_segment(
            [scale.point(402.0, 297.0), scale.point(646.0, 297.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        if put_button(
            ui,
            scale.rect(TELEMETRY_CONTINUE_RECT),
            scale,
            "Продолжить с последней сводки",
            if self.continue_from_last {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.continue_from_last = !self.continue_from_last;
        }

        let can_run = !self.task.running;
        let response = put_button(
            ui,
            scale.rect(TELEMETRY_ACTION_RECT),
            scale,
            "Выгрузить данные",
            if can_run {
                ButtonVisuals::danger(palette)
            } else {
                ButtonVisuals::disabled(palette)
            },
            18.0,
            20.0,
        );

        if can_run && response.clicked() {
            let request = telemetry::Request {
                year: self.year,
                continue_from_last: self.continue_from_last,
            };
            let (tx, rx) = mpsc::channel();
            self.task.start(rx, "Запущен сбор телеметрии...");
            spawn_result_task(tx, "Телеметрия", move || {
                telemetry::execute(&request)
            });
        }
    }

    fn poll(&mut self) {
        self.task.update_with(finish_with_paths, || {
            "Фоновая задача телеметрии была прервана.".to_string()
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
