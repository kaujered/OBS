use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use egui::{Align2, Stroke};

use crate::cli::Cli;
use crate::domain::Mest;
use crate::modes::gdi;
use crate::ui::app::{
    TaskState, day_options, default_day, default_month, default_selected_mests, default_year,
    finish_with_paths, month_options, spawn_result_task, year_options,
};
use crate::ui::components::{
    ButtonVisuals, paint_text, put_button, put_button_rich, put_scrollable_combo_box,
    show_inner_section,
};
use crate::ui::theme::{LayoutScale, Palette, RectSpec};

use super::{
    GDI_ACTION_RECT, GDI_CHARTS_RECT, GDI_DEBUG_RECT, REGULAR_LEFT_SECTION_RECT,
    REGULAR_MAIN_CARD_RECT, draw_outer_panel, primary_action_label, primary_action_visuals,
    render_regular_mest_card,
};

pub(crate) struct GdiScreen {
    selected: BTreeSet<Mest>,
    year: i32,
    month: u32,
    day: u32,
    test_mode: bool,
    debug_mode: bool,
    charts_mode: bool,
    only_last: bool,
    include_rejected: bool,
    bngkm_source: gdi::DataSourceChoice,
    hgkm_source: gdi::DataSourceChoice,
    task: TaskState<Vec<PathBuf>>,
    /// When true, the task is started automatically on the first render frame.
    pending_start: bool,
    /// YYYYMMDD date key for the FILTRED_GDI sheet; None means no filtered sheet.
    filtr_date: Option<i64>,
}

impl Default for GdiScreen {
    fn default() -> Self {
        Self {
            selected: default_selected_mests(),
            year: default_year(),
            month: default_month(),
            day: default_day(),
            test_mode: false,
            debug_mode: false,
            charts_mode: false,
            only_last: true,
            include_rejected: false,
            bngkm_source: gdi::DataSourceChoice::Kots,
            hgkm_source: gdi::DataSourceChoice::Kots,
            task: TaskState::default(),
            pending_start: false,
            filtr_date: None,
        }
    }
}

impl GdiScreen {
    /// Build a pre-filled screen from CLI flags and schedule an automatic run.
    pub(crate) fn from_cli(cli: &Cli) -> Self {
        let (year, month, day) = cli.parsed_date();
        let selected = cli.parsed_mests().into_iter().collect();
        Self {
            selected,
            year,
            month,
            day,
            test_mode: cli.test,
            debug_mode: false,
            charts_mode: cli.graph,
            only_last: cli.only_last,
            include_rejected: cli.brak,
            bngkm_source: if cli.xlsx {
                gdi::DataSourceChoice::Kots
            } else {
                gdi::DataSourceChoice::Dbf
            },
            hgkm_source: gdi::DataSourceChoice::Kots,
            task: TaskState::default(),
            pending_start: true,
            filtr_date: cli.parsed_filtr_date(),
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
            "Для ГДИ",
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
            "С выбранной даты",
            14.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            510.0,
            448.0,
            Align2::RIGHT_CENTER,
            "Год",
            14.0,
            palette.text,
        );
        let year_choices = year_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(520.0, 432.0, 100.0, 28.0)),
            scale,
            palette,
            "gdi_year_combo",
            &mut self.year,
            year_choices,
            |year| year.to_string(),
        );
        paint_text(
            ui,
            scale,
            510.0,
            490.0,
            Align2::RIGHT_CENTER,
            "Месяц",
            14.0,
            palette.text,
        );
        let month_choices = month_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(520.0, 474.0, 100.0, 28.0)),
            scale,
            palette,
            "gdi_month_combo",
            &mut self.month,
            month_choices,
            |month| format!("{month:02}"),
        );
        paint_text(
            ui,
            scale,
            510.0,
            532.0,
            Align2::RIGHT_CENTER,
            "День",
            14.0,
            palette.text,
        );
        let day_choices = day_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(520.0, 516.0, 100.0, 28.0)),
            scale,
            palette,
            "gdi_day_combo",
            &mut self.day,
            day_choices,
            |day| format!("{day:02}"),
        );

        ui.painter().line_segment(
            [scale.point(402.0, 559.0), scale.point(646.0, 559.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        paint_text(
            ui,
            scale,
            524.0,
            577.0,
            Align2::CENTER_CENTER,
            "Исходные данные",
            14.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            467.0,
            609.0,
            Align2::CENTER_CENTER,
            "для БНГКМ",
            13.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            581.0,
            609.0,
            Align2::CENTER_CENTER,
            "для ХГКМ",
            13.0,
            palette.text,
        );

        ui.painter().line_segment(
            [scale.point(531.0, 622.0), scale.point(531.0, 744.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        // These source toggles drive the native DBF/KOTS routing inside `modes::gdi`.
        if put_button(
            ui,
            scale.rect(RectSpec::new(417.0, 645.0, 98.0, 35.0)),
            scale,
            "DBF",
            if self.bngkm_source == gdi::DataSourceChoice::Dbf {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.bngkm_source = gdi::DataSourceChoice::Dbf;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(417.0, 702.0, 98.0, 35.0)),
            scale,
            "XLSX",
            if self.bngkm_source == gdi::DataSourceChoice::Kots {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.bngkm_source = gdi::DataSourceChoice::Kots;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(546.0, 645.0, 98.0, 35.0)),
            scale,
            "DBF",
            if self.hgkm_source == gdi::DataSourceChoice::Dbf {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.hgkm_source = gdi::DataSourceChoice::Dbf;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(546.0, 702.0, 98.0, 35.0)),
            scale,
            "XLSX",
            if self.hgkm_source == gdi::DataSourceChoice::Kots {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.hgkm_source = gdi::DataSourceChoice::Kots;
        }

        ui.painter().line_segment(
            [scale.point(402.0, 753.0), scale.point(646.0, 753.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );
        paint_text(
            ui,
            scale,
            524.0,
            776.0,
            Align2::CENTER_CENTER,
            "Опции",
            14.0,
            palette.text,
        );

        if put_button(
            ui,
            scale.rect(RectSpec::new(379.0, 798.0, 290.0, 35.0)),
            scale,
            "Только последние исследования",
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
            scale.rect(RectSpec::new(377.0, 847.0, 290.0, 35.0)),
            scale,
            "Выгружать отбракованные",
            if self.include_rejected {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.include_rejected = !self.include_rejected;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(377.0, 894.0, 290.0, 35.0)),
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

        let has_selected_mest = !self.selected.is_empty();
        let can_run = !self.task.running && has_selected_mest;
        let action = put_button_rich(
            ui,
            scale.rect(GDI_ACTION_RECT),
            scale,
            primary_action_label(has_selected_mest),
            primary_action_visuals(palette, has_selected_mest, can_run),
            18.0,
            20.0,
        );
        if can_run && action.clicked() {
            self.start_task();
        }

        if put_button(
            ui,
            scale.rect(GDI_CHARTS_RECT),
            scale,
            "ГРАФИКИ",
            if self.charts_mode {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.charts_mode = !self.charts_mode;
        }

        if put_button(
            ui,
            scale.rect(GDI_DEBUG_RECT),
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
    }

    /// Start the GDI export task.
    fn start_task(&mut self) {
        let request = gdi::Request {
            mests: self.selected.iter().copied().collect(),
            year: self.year,
            month: self.month,
            day: self.day,
            test_mode: self.test_mode,
            debug_mode: self.debug_mode,
            only_last: self.only_last,
            include_rejected: self.include_rejected,
            bngkm_source: self.bngkm_source,
            hgkm_source: self.hgkm_source,
            export_charts: self.charts_mode,
            filter_date: self.filtr_date,
        };
        let (tx, rx) = mpsc::channel();
        self.task.start(rx, "Запущена обработка ГДИ...");
        spawn_result_task(tx, "ГДИ", move || gdi::execute(&request));
    }

    fn poll(&mut self) {
        self.task.update_with(finish_with_paths, || {
            "Фоновая задача ГДИ была прервана.".to_string()
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
