//! Общая отрисовка, одинаковая на нескольких вкладках: внешняя панель,
//! главная кнопка и карточки выбора месторождений.

use std::collections::BTreeSet;

use egui::{Align2, RichText};

use crate::domain::Mest;
use crate::ui::components::{ButtonVisuals, paint_text, put_button, show_card};
use crate::ui::kinds::ModeKind;
use crate::ui::screens::layout::{
    BATCH_MODULE_CARD_RECT, BATCH_TOP_MEST_CARD_RECT, REGULAR_MEST_CARD_RECT,
};
use crate::ui::theme::{CARD_RADIUS, LayoutScale, Palette, RectSpec, SMALL_BUTTON_RADIUS};

pub(crate) fn draw_outer_panel(
    ui: &mut egui::Ui,
    scale: &LayoutScale,
    palette: Palette,
    rect: RectSpec,
    title: &str,
) {
    show_card(
        ui,
        scale.rect(rect),
        scale,
        palette,
        palette.section,
        CARD_RADIUS,
        12.0,
        true,
        |_| {},
    );
    paint_text(
        ui,
        scale,
        rect.x + rect.width / 2.0,
        rect.y + 20.0,
        Align2::CENTER_CENTER,
        title,
        16.0,
        palette.text,
    );
}

pub(crate) fn primary_action_label(has_selected_mest: bool) -> RichText {
    if has_selected_mest {
        RichText::new("Выгрузить данные")
    } else {
        RichText::new("Выберите месторождение").strong()
    }
}

pub(crate) fn primary_action_visuals(
    palette: Palette,
    has_selected_mest: bool,
    can_run: bool,
) -> ButtonVisuals {
    if can_run {
        ButtonVisuals::danger(palette)
    } else if has_selected_mest {
        ButtonVisuals::disabled(palette)
    } else {
        ButtonVisuals {
            text: palette.danger,
            ..ButtonVisuals::disabled(palette)
        }
    }
}

pub(crate) fn render_regular_mest_card(
    ui: &mut egui::Ui,
    scale: &LayoutScale,
    palette: Palette,
    selected: &mut BTreeSet<Mest>,
) {
    const CHIP_RECTS: [(Mest, RectSpec); 7] = [
        (Mest::Mgpu, RectSpec::new(369.0, 128.0, 205.0, 37.0)),
        (
            Mest::YungkmSenoman,
            RectSpec::new(588.0, 128.0, 205.0, 37.0),
        ),
        (Mest::Bngkm, RectSpec::new(807.0, 128.0, 205.0, 37.0)),
        (Mest::MgpuNyda, RectSpec::new(369.0, 179.0, 205.0, 37.0)),
        (Mest::YungkmAptAlb, RectSpec::new(588.0, 179.0, 205.0, 37.0)),
        (Mest::Hgkm, RectSpec::new(807.0, 179.0, 205.0, 37.0)),
        (Mest::Yangkm, RectSpec::new(588.0, 230.0, 205.0, 37.0)),
    ];

    show_card(
        ui,
        scale.rect(REGULAR_MEST_CARD_RECT),
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
        selected.extend(Mest::ALL);
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
        selected.clear();
    }

    for (mest, rect) in CHIP_RECTS {
        let is_active = selected.contains(&mest);
        if put_button(
            ui,
            scale.rect(rect),
            scale,
            mest.ui_label(),
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
                selected.remove(&mest);
            } else {
                selected.insert(mest);
            }
        }
    }
}

pub(crate) fn render_batch_mest_card(
    ui: &mut egui::Ui,
    scale: &LayoutScale,
    palette: Palette,
    selected: &mut BTreeSet<Mest>,
) {
    const CHIP_RECTS: [(Mest, RectSpec); 7] = [
        (Mest::Mgpu, RectSpec::new(369.0, 128.0, 130.0, 37.0)),
        (
            Mest::YungkmSenoman,
            RectSpec::new(512.0, 128.0, 209.0, 37.0),
        ),
        (Mest::Bngkm, RectSpec::new(734.0, 128.0, 132.0, 37.0)),
        (Mest::Yangkm, RectSpec::new(880.0, 128.0, 131.0, 37.0)),
        (Mest::MgpuNyda, RectSpec::new(369.0, 179.0, 130.0, 37.0)),
        (Mest::YungkmAptAlb, RectSpec::new(512.0, 179.0, 209.0, 37.0)),
        (Mest::Hgkm, RectSpec::new(734.0, 179.0, 132.0, 37.0)),
    ];

    show_card(
        ui,
        scale.rect(BATCH_TOP_MEST_CARD_RECT),
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
        383.0,
        107.0,
        Align2::LEFT_CENTER,
        "Месторождения",
        16.0,
        palette.text,
    );

    if put_button(
        ui,
        scale.rect(RectSpec::new(515.0, 96.0, 94.0, 22.0)),
        scale,
        "Выбрать все",
        ButtonVisuals::neutral(palette),
        12.0,
        SMALL_BUTTON_RADIUS,
    )
    .clicked()
    {
        selected.extend(Mest::ALL);
    }

    if put_button(
        ui,
        scale.rect(RectSpec::new(627.0, 96.0, 94.0, 22.0)),
        scale,
        "Очистить",
        ButtonVisuals::neutral(palette),
        12.0,
        SMALL_BUTTON_RADIUS,
    )
    .clicked()
    {
        selected.clear();
    }

    for (mest, rect) in CHIP_RECTS {
        let is_active = selected.contains(&mest);
        if put_button(
            ui,
            scale.rect(rect),
            scale,
            mest.ui_label(),
            if is_active {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            13.5,
            SMALL_BUTTON_RADIUS,
        )
        .clicked()
        {
            if is_active {
                selected.remove(&mest);
            } else {
                selected.insert(mest);
            }
        }
    }
}

pub(crate) fn render_batch_module_card(
    ui: &mut egui::Ui,
    scale: &LayoutScale,
    palette: Palette,
    run_gdi: &mut bool,
    run_ppl: &mut bool,
    run_ss: &mut bool,
    run_ved: &mut bool,
) {
    const MODULES: [(&str, RectSpec, ModeKind); 4] = [
        (
            "ГДИ",
            RectSpec::new(366.0, 283.0, 150.0, 35.0),
            ModeKind::Gdi,
        ),
        (
            "Статика",
            RectSpec::new(530.0, 283.0, 150.0, 35.0),
            ModeKind::Ppl,
        ),
        (
            "Сводка",
            RectSpec::new(694.0, 283.0, 150.0, 35.0),
            ModeKind::Ss,
        ),
        (
            "Ведомость",
            RectSpec::new(858.0, 283.0, 150.0, 35.0),
            ModeKind::Ved,
        ),
    ];

    show_card(
        ui,
        scale.rect(BATCH_MODULE_CARD_RECT),
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
        406.0,
        264.0,
        Align2::LEFT_CENTER,
        "Модули",
        16.0,
        palette.text,
    );

    if put_button(
        ui,
        scale.rect(RectSpec::new(515.0, 253.0, 94.0, 22.0)),
        scale,
        "Выбрать все",
        ButtonVisuals::neutral(palette),
        12.0,
        SMALL_BUTTON_RADIUS,
    )
    .clicked()
    {
        *run_gdi = true;
        *run_ppl = true;
        *run_ss = true;
        *run_ved = true;
    }

    if put_button(
        ui,
        scale.rect(RectSpec::new(627.0, 253.0, 94.0, 22.0)),
        scale,
        "Очистить",
        ButtonVisuals::neutral(palette),
        12.0,
        SMALL_BUTTON_RADIUS,
    )
    .clicked()
    {
        *run_gdi = false;
        *run_ppl = false;
        *run_ss = false;
        *run_ved = false;
    }

    for (label, rect, kind) in MODULES {
        let active = match kind {
            ModeKind::Gdi => *run_gdi,
            ModeKind::Ppl => *run_ppl,
            ModeKind::Ss => *run_ss,
            ModeKind::Ved => *run_ved,
            _ => false,
        };

        if put_button(
            ui,
            scale.rect(rect),
            scale,
            label,
            if active {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            14.0,
            SMALL_BUTTON_RADIUS,
        )
        .clicked()
        {
            match kind {
                ModeKind::Gdi => *run_gdi = !*run_gdi,
                ModeKind::Ppl => *run_ppl = !*run_ppl,
                ModeKind::Ss => *run_ss = !*run_ss,
                ModeKind::Ved => *run_ved = !*run_ved,
                _ => {}
            }
        }
    }
}
