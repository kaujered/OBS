use chrono::{Datelike, Local};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use egui::{Align2, Stroke};
use rayon::prelude::*;

use crate::domain::Mest;
use crate::modes::{gdi, ppl, ss, ved};
use crate::ui::app::{
    TaskState, day_options, default_day, default_month, default_selected_mests, default_year,
    format_paths, month_options, spawn_result_task, year_options,
};
use crate::ui::components::{
    ButtonVisuals, paint_text, put_button, put_button_rich, put_scrollable_combo_box,
    show_inner_section,
};
use crate::ui::theme::{LayoutScale, Palette, RectSpec};

use super::{
    BATCH_ACTION_RECT, BATCH_LEFT_SECTION_RECT, BATCH_MAIN_CARD_RECT, BATCH_RIGHT_SECTION_RECT,
    draw_outer_panel, primary_action_label, primary_action_visuals, render_batch_mest_card,
    render_batch_module_card,
};

#[derive(Debug, Clone, Copy)]
enum BatchJob {
    Gdi,
    Ppl,
    Ss,
    Ved,
}

#[derive(Debug, Clone)]
struct BatchRequest {
    mests: Vec<Mest>,
    year: i32,
    month: u32,
    gdi_year: i32,
    gdi_month: u32,
    gdi_day: u32,
    test_mode: bool,
    debug_mode: bool,
    run_gdi: bool,
    run_ppl: bool,
    run_ss: bool,
    run_ved: bool,
    ppl_only_last: bool,
    ss_only_last: bool,
    gdi_only_last: bool,
    gdi_include_rejected: bool,
    bngkm_source: gdi::DataSourceChoice,
    hgkm_source: gdi::DataSourceChoice,
}

#[derive(Debug, Clone)]
struct BatchExecutionReport {
    status_text: String,
    created_paths: Vec<PathBuf>,
}

#[derive(Debug)]
struct BatchModuleOutcome {
    label: &'static str,
    result: Result<Vec<PathBuf>>,
}

#[derive(Debug, Default)]
struct StatusParts {
    text: String,
}

impl StatusParts {
    fn push(&mut self, part: String) {
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        self.text.push_str(&part);
    }

    fn finish(self) -> String {
        self.text
    }
}

#[derive(Debug)]
struct BatchAggregateEntry {
    index: usize,
    detail: String,
    created_paths: Vec<PathBuf>,
}

#[derive(Debug, Default)]
struct BatchAggregate {
    ok_count: usize,
    error_count: usize,
    created_files: usize,
    entries: Vec<BatchAggregateEntry>,
}

impl BatchAggregate {
    fn push(&mut self, index: usize, outcome: BatchModuleOutcome) {
        match outcome.result {
            Ok(paths) => {
                self.ok_count += 1;
                self.created_files += paths.len();
                self.entries.push(BatchAggregateEntry {
                    index,
                    detail: format!("{}\n{}", outcome.label, format_paths(&paths)),
                    created_paths: paths,
                });
            }
            Err(error) => {
                self.error_count += 1;
                self.entries.push(BatchAggregateEntry {
                    index,
                    detail: format!("{}\nОшибка: {error:#}", outcome.label),
                    created_paths: Vec::new(),
                });
            }
        }
    }

    fn merge(&mut self, mut other: Self) {
        self.ok_count += other.ok_count;
        self.error_count += other.error_count;
        self.created_files += other.created_files;
        self.entries.append(&mut other.entries);
    }

    fn into_parts(mut self) -> (usize, usize, usize, Vec<PathBuf>, String) {
        self.entries.sort_by_key(|entry| entry.index);
        let mut created_paths = Vec::new();
        let mut details = StatusParts::default();
        for entry in self.entries {
            created_paths.extend(entry.created_paths);
            details.push(entry.detail);
        }
        (
            self.ok_count,
            self.error_count,
            self.created_files,
            created_paths,
            details.finish(),
        )
    }
}

pub(crate) struct BatchScreen {
    selected: std::collections::BTreeSet<Mest>,
    year: i32,
    month: u32,
    gdi_year: i32,
    gdi_month: u32,
    gdi_day: u32,
    test_mode: bool,
    debug_mode: bool,
    run_gdi: bool,
    run_ppl: bool,
    run_ss: bool,
    run_ved: bool,
    ppl_only_last: bool,
    ss_only_last: bool,
    gdi_only_last: bool,
    gdi_include_rejected: bool,
    bngkm_source: gdi::DataSourceChoice,
    hgkm_source: gdi::DataSourceChoice,
    task: TaskState<BatchExecutionReport>,
}

impl Default for BatchScreen {
    fn default() -> Self {
        Self {
            selected: default_selected_mests(),
            year: default_year(),
            month: Local::now().month(),
            gdi_year: default_year(),
            gdi_month: default_month(),
            gdi_day: default_day(),
            test_mode: false,
            debug_mode: false,
            run_gdi: true,
            run_ppl: true,
            run_ss: true,
            run_ved: true,
            ppl_only_last: true,
            ss_only_last: true,
            gdi_only_last: true,
            gdi_include_rejected: false,
            bngkm_source: gdi::DataSourceChoice::Kots,
            hgkm_source: gdi::DataSourceChoice::Kots,
            task: TaskState::default(),
        }
    }
}

impl BatchScreen {
    pub(crate) fn render(&mut self, ui: &mut egui::Ui, scale: &LayoutScale, palette: Palette) {
        self.poll();

        render_batch_mest_card(ui, scale, palette, &mut self.selected);
        render_batch_module_card(
            ui,
            scale,
            palette,
            &mut self.run_gdi,
            &mut self.run_ppl,
            &mut self.run_ss,
            &mut self.run_ved,
        );

        draw_outer_panel(ui, scale, palette, BATCH_MAIN_CARD_RECT, "Параметры");
        show_inner_section(
            ui,
            scale.rect(BATCH_LEFT_SECTION_RECT),
            scale,
            palette,
            |_| {},
        );
        show_inner_section(
            ui,
            scale.rect(BATCH_RIGHT_SECTION_RECT),
            scale,
            palette,
            |_| {},
        );

        self.render_left_panel(ui, scale, palette);
        self.render_right_panel(ui, scale, palette);

        let has_selected_mest = !self.selected.is_empty();
        let can_run = !self.task.running && has_selected_mest && self.selected_module_count() > 0;
        let response = put_button_rich(
            ui,
            scale.rect(BATCH_ACTION_RECT),
            scale,
            primary_action_label(has_selected_mest),
            primary_action_visuals(palette, has_selected_mest, can_run),
            18.0,
            20.0,
        );

        if can_run && response.clicked() {
            let request = self.build_request();
            let (tx, rx) = mpsc::channel();
            self.task.start(rx, "Запущен пакетный режим...");
            spawn_result_task(tx, "Все и сразу", move || execute_batch(&request));
        }
    }

    fn render_left_panel(&mut self, ui: &mut egui::Ui, scale: &LayoutScale, palette: Palette) {
        paint_text(
            ui,
            scale,
            524.0,
            419.0,
            Align2::CENTER_CENTER,
            "Для ГДИ",
            15.0,
            palette.text,
        );
        ui.painter().line_segment(
            [scale.point(402.0, 437.0), scale.point(646.0, 437.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        paint_text(
            ui,
            scale,
            524.0,
            453.0,
            Align2::CENTER_CENTER,
            "С выбранной даты",
            14.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            512.0,
            495.0,
            Align2::RIGHT_CENTER,
            "Год",
            14.0,
            palette.text,
        );
        let year_choices = year_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(522.0, 479.0, 100.0, 28.0)),
            scale,
            palette,
            "batch_gdi_year_combo",
            &mut self.gdi_year,
            year_choices,
            |year| year.to_string(),
        );
        paint_text(
            ui,
            scale,
            512.0,
            537.0,
            Align2::RIGHT_CENTER,
            "Месяц",
            14.0,
            palette.text,
        );
        let month_choices = month_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(522.0, 521.0, 100.0, 28.0)),
            scale,
            palette,
            "batch_gdi_month_combo",
            &mut self.gdi_month,
            month_choices,
            |month| format!("{month:02}"),
        );
        paint_text(
            ui,
            scale,
            512.0,
            579.0,
            Align2::RIGHT_CENTER,
            "День",
            14.0,
            palette.text,
        );
        let day_choices = day_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(522.0, 563.0, 100.0, 28.0)),
            scale,
            palette,
            "batch_gdi_day_combo",
            &mut self.gdi_day,
            day_choices,
            |day| format!("{day:02}"),
        );

        ui.painter().line_segment(
            [scale.point(402.0, 602.0), scale.point(646.0, 602.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );
        paint_text(
            ui,
            scale,
            524.0,
            623.0,
            Align2::CENTER_CENTER,
            "Исходные данные",
            14.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            469.0,
            651.0,
            Align2::CENTER_CENTER,
            "для БНГКМ",
            13.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            580.0,
            651.0,
            Align2::CENTER_CENTER,
            "для ХГКМ",
            13.0,
            palette.text,
        );
        ui.painter().line_segment(
            [scale.point(531.0, 649.0), scale.point(531.0, 771.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        if put_button(
            ui,
            scale.rect(RectSpec::new(417.0, 670.0, 98.0, 35.0)),
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
            scale.rect(RectSpec::new(417.0, 720.0, 98.0, 35.0)),
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
            scale.rect(RectSpec::new(545.0, 670.0, 98.0, 35.0)),
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
            scale.rect(RectSpec::new(545.0, 720.0, 98.0, 35.0)),
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
            [scale.point(402.0, 770.0), scale.point(646.0, 770.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );
        paint_text(
            ui,
            scale,
            524.0,
            793.0,
            Align2::CENTER_CENTER,
            "Опции",
            14.0,
            palette.text,
        );
        if put_button(
            ui,
            scale.rect(RectSpec::new(379.0, 810.0, 290.0, 35.0)),
            scale,
            "Только последние исследования",
            if self.gdi_only_last {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.gdi_only_last = !self.gdi_only_last;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(377.0, 855.0, 290.0, 35.0)),
            scale,
            "Выгружать отбракованные",
            if self.gdi_include_rejected {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.gdi_include_rejected = !self.gdi_include_rejected;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(377.0, 900.0, 290.0, 35.0)),
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
    }

    fn render_right_panel(&mut self, ui: &mut egui::Ui, scale: &LayoutScale, palette: Palette) {
        paint_text(
            ui,
            scale,
            854.0,
            419.0,
            Align2::CENTER_CENTER,
            "Для остальных модулей",
            15.0,
            palette.text,
        );
        ui.painter().line_segment(
            [scale.point(733.0, 437.0), scale.point(974.0, 437.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );

        paint_text(
            ui,
            scale,
            854.0,
            453.0,
            Align2::CENTER_CENTER,
            "Дата",
            14.0,
            palette.text,
        );
        paint_text(
            ui,
            scale,
            840.0,
            495.0,
            Align2::RIGHT_CENTER,
            "Год",
            14.0,
            palette.text,
        );
        let year_choices = year_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(850.0, 479.0, 100.0, 28.0)),
            scale,
            palette,
            "batch_year_combo",
            &mut self.year,
            year_choices,
            |year| year.to_string(),
        );
        paint_text(
            ui,
            scale,
            840.0,
            537.0,
            Align2::RIGHT_CENTER,
            "Месяц",
            14.0,
            palette.text,
        );
        let month_choices = month_options();
        put_scrollable_combo_box(
            ui,
            scale.rect(RectSpec::new(850.0, 521.0, 100.0, 28.0)),
            scale,
            palette,
            "batch_month_combo",
            &mut self.month,
            month_choices,
            |month| format!("{month:02}"),
        );

        ui.painter().line_segment(
            [scale.point(733.0, 560.0), scale.point(974.0, 560.0)],
            Stroke::new(scale.px(1.0), palette.border),
        );
        paint_text(
            ui,
            scale,
            854.0,
            583.0,
            Align2::CENTER_CENTER,
            "Опции",
            14.0,
            palette.text,
        );

        // These toggles fan out into `ppl::Request.only_last` and `ss::Request.only_last`.
        if put_button(
            ui,
            scale.rect(RectSpec::new(707.0, 600.0, 290.0, 35.0)),
            scale,
            "Последняя статика",
            if self.ppl_only_last {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.ppl_only_last = !self.ppl_only_last;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(707.0, 645.0, 290.0, 35.0)),
            scale,
            "Последняя сводка",
            if self.ss_only_last {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.0,
            14.0,
        )
        .clicked()
        {
            self.ss_only_last = !self.ss_only_last;
        }
        if put_button(
            ui,
            scale.rect(RectSpec::new(707.0, 690.0, 290.0, 35.0)),
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
            scale.rect(RectSpec::new(707.0, 735.0, 290.0, 35.0)),
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

    fn selected_module_count(&self) -> usize {
        [self.run_gdi, self.run_ppl, self.run_ss, self.run_ved]
            .into_iter()
            .filter(|selected| *selected)
            .count()
    }

    fn build_request(&self) -> BatchRequest {
        BatchRequest {
            mests: self.selected.iter().copied().collect(),
            year: self.year,
            month: self.month,
            gdi_year: self.gdi_year,
            gdi_month: self.gdi_month,
            gdi_day: self.gdi_day,
            test_mode: self.test_mode,
            debug_mode: self.debug_mode,
            run_gdi: self.run_gdi,
            run_ppl: self.run_ppl,
            run_ss: self.run_ss,
            run_ved: self.run_ved,
            ppl_only_last: self.ppl_only_last,
            ss_only_last: self.ss_only_last,
            gdi_only_last: self.gdi_only_last,
            gdi_include_rejected: self.gdi_include_rejected,
            bngkm_source: self.bngkm_source,
            hgkm_source: self.hgkm_source,
        }
    }

    fn poll(&mut self) {
        self.task.update_with(
            |report| (report.status_text, report.created_paths),
            || "Фоновая задача пакетного запуска была прервана.".to_string(),
        );
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

fn execute_batch(request: &BatchRequest) -> Result<BatchExecutionReport> {
    if request.mests.is_empty() {
        return Err(anyhow!("Выберите хотя бы одно месторождение."));
    }

    let jobs = selected_jobs(request);
    if jobs.is_empty() {
        return Err(anyhow!("Выберите хотя бы один модуль."));
    }

    let aggregate = jobs
        .into_par_iter()
        .enumerate()
        .fold(BatchAggregate::default, |mut aggregate, (index, job)| {
            aggregate.push(index, execute_batch_job(job, request));
            aggregate
        })
        .reduce(BatchAggregate::default, |mut left, right| {
            left.merge(right);
            left
        });
    let (ok_count, error_count, created_files, created_paths, details) = aggregate.into_parts();

    let summary = if error_count == 0 {
        format!("Готово. Модулей: {}. Файлов: {}.", ok_count, created_files)
    } else {
        format!(
            "Завершено с ошибками. Успешно: {}. Ошибок: {}. Файлов: {}.",
            ok_count, error_count, created_files
        )
    };

    let mut status_text = summary;
    if !details.is_empty() {
        status_text.push_str("\n\n");
        status_text.push_str(&details);
    }

    Ok(BatchExecutionReport {
        status_text,
        created_paths,
    })
}

fn selected_jobs(request: &BatchRequest) -> Vec<BatchJob> {
    let mut jobs = Vec::new();
    if request.run_gdi {
        jobs.push(BatchJob::Gdi);
    }
    if request.run_ppl {
        jobs.push(BatchJob::Ppl);
    }
    if request.run_ss {
        jobs.push(BatchJob::Ss);
    }
    if request.run_ved {
        jobs.push(BatchJob::Ved);
    }
    jobs
}

fn execute_batch_job(job: BatchJob, request: &BatchRequest) -> BatchModuleOutcome {
    match job {
        BatchJob::Gdi => BatchModuleOutcome {
            label: "ГДИ",
            result: gdi::execute(&gdi::Request {
                mests: request.mests.clone(),
                year: request.gdi_year,
                month: request.gdi_month,
                day: request.gdi_day,
                test_mode: request.test_mode,
                debug_mode: request.debug_mode,
                only_last: request.gdi_only_last,
                include_rejected: request.gdi_include_rejected,
                bngkm_source: request.bngkm_source,
                hgkm_source: request.hgkm_source,
                export_charts: false,
                filter_date: None,
            }),
        },
        BatchJob::Ppl => BatchModuleOutcome {
            label: "Статика",
            result: ppl::execute(&ppl::Request {
                mests: request.mests.clone(),
                year: request.year,
                test_mode: request.test_mode,
                debug_mode: request.debug_mode,
                only_last: request.ppl_only_last,
            }),
        },
        BatchJob::Ss => BatchModuleOutcome {
            label: "Сводка",
            result: ss::execute(&ss::Request {
                mests: request.mests.clone(),
                year: request.year,
                month: request.month,
                test_mode: request.test_mode,
                debug_mode: request.debug_mode,
                only_last: request.ss_only_last,
            }),
        },
        BatchJob::Ved => BatchModuleOutcome {
            label: "Ведомость",
            result: ved::execute(&ved::Request {
                mests: request.mests.clone(),
                year: request.year,
                month: request.month,
                test_mode: request.test_mode,
                debug_mode: request.debug_mode,
                correct_work_params: true,
            }),
        },
    }
}
