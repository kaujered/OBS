use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use egui::{
    self, Align, Color32, CornerRadius, FontId, Frame, Id, Layout, Margin, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, UiBuilder,
};

use crate::ui::kinds::{ModeKind, ThemeMode};

use super::controls::{ButtonVisuals, put_button};
use crate::ui::theme::{
    BODY_SIZE, BUTTON_RADIUS, CARD_RADIUS, LayoutScale, OUTER_CARD_RADIUS, Palette, SECTION_RADIUS,
    SMALL_BUTTON_RADIUS, TITLE_SIZE,
};

pub(crate) fn paint_shell(ui: &mut egui::Ui, rect: Rect, scale: &LayoutScale, palette: Palette) {
    let painter = ui.painter();
    painter.rect(
        rect.expand(scale.px(6.0)),
        CornerRadius::same(scale.radius(OUTER_CARD_RADIUS + 8.0)),
        Color32::TRANSPARENT,
        Stroke::NONE,
        egui::epaint::StrokeKind::Outside,
    );
    painter.rect_filled(
        rect,
        CornerRadius::same(scale.radius(OUTER_CARD_RADIUS)),
        palette.shell,
    );
    painter.add(egui::epaint::RectShape::filled(
        rect.expand(scale.px(1.5)),
        CornerRadius::same(scale.radius(OUTER_CARD_RADIUS)),
        palette.shell,
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn show_card<R>(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    fill: Color32,
    radius: f32,
    padding: f32,
    shadow: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let inner_size = egui::vec2(
        (rect.width() - scale.px(padding) * 2.0).max(0.0),
        (rect.height() - scale.px(padding) * 2.0).max(0.0),
    );
    let frame = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(scale.px(1.0), palette.border))
        .corner_radius(CornerRadius::same(scale.radius(radius)))
        .inner_margin(Margin::same(scale.margin(padding)))
        .shadow(if shadow {
            egui::epaint::Shadow {
                offset: [0, scale.px(4.0).round() as i8],
                blur: scale.px(18.0).round() as u8,
                spread: 0,
                color: palette.shadow,
            }
        } else {
            egui::epaint::Shadow::NONE
        });

    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(scale.px(8.0), scale.px(8.0));
            frame
                .show(ui, |ui| {
                    ui.set_min_size(inner_size);
                    add_contents(ui)
                })
                .inner
        },
    )
    .inner
}

pub(crate) fn show_inner_section<R>(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    show_card(
        ui,
        rect,
        scale,
        palette,
        palette.card,
        SECTION_RADIUS,
        18.0,
        false,
        add_contents,
    )
}

pub(crate) fn render_brand_card(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    screen_title: &str,
) {
    show_card(
        ui,
        rect,
        scale,
        palette,
        palette.card,
        CARD_RADIUS,
        18.0,
        true,
        |ui| {
            ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
                ui.add_space(scale.px(18.0));
                ui.label(
                    RichText::new("OneBigScript")
                        .font(FontId::proportional(scale.px(24.0)))
                        .color(palette.text),
                );
                ui.add_space(scale.px(4.0));
                ui.separator();
                ui.add_space(scale.px(6.0));
                ui.label(
                    RichText::new(screen_title)
                        .font(FontId::proportional(scale.px(17.0)))
                        .color(palette.text_soft),
                );
            });
        },
    );
}

pub(crate) fn render_sidebar(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    current: ModeKind,
) -> Option<ModeKind> {
    // Семь кнопок делят ту же высоту карточки, что раньше делили шесть,
    // поэтому шаг и высота пересчитаны под новый набор.
    const BUTTONS: [(ModeKind, f32); 7] = [
        (ModeKind::Batch, 258.0),
        (ModeKind::Gdi, 335.5),
        (ModeKind::Ppl, 413.0),
        (ModeKind::Ss, 490.5),
        (ModeKind::Ved, 568.0),
        (ModeKind::Telemetry, 645.5),
        (ModeKind::Gdm, 723.0),
    ];

    let mut next = None;
    show_card(
        ui,
        rect,
        scale,
        palette,
        palette.card,
        CARD_RADIUS,
        14.0,
        true,
        |_| {},
    );

    for (mode, y) in BUTTONS {
        let response = put_button(
            ui,
            egui::Rect::from_min_size(scale.point(113.0, y), scale.size(208.0, 65.0)),
            scale,
            mode.ui_label(),
            if current == mode {
                ButtonVisuals::active(palette)
            } else {
                ButtonVisuals::neutral(palette)
            },
            17.0,
            BUTTON_RADIUS,
        );
        if response.clicked() {
            next = Some(mode);
        }
    }

    next
}

pub(crate) fn render_theme_card(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    theme: &mut ThemeMode,
) {
    show_card(
        ui,
        rect,
        scale,
        palette,
        palette.card,
        CARD_RADIUS,
        14.0,
        true,
        |ui| {
            ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
                ui.label(
                    RichText::new("Выбор темы")
                        .font(FontId::proportional(scale.px(TITLE_SIZE)))
                        .color(palette.text),
                );
            });
        },
    );

    if put_button(
        ui,
        egui::Rect::from_min_size(scale.point(113.0, 855.0), scale.size(208.0, 38.0)),
        scale,
        "Светлая",
        if *theme == ThemeMode::Light {
            ButtonVisuals::active(palette)
        } else {
            ButtonVisuals::neutral(palette)
        },
        16.0,
        SMALL_BUTTON_RADIUS,
    )
    .clicked()
    {
        *theme = ThemeMode::Light;
    }

    if put_button(
        ui,
        egui::Rect::from_min_size(scale.point(113.0, 907.0), scale.size(208.0, 38.0)),
        scale,
        "Темная",
        if *theme == ThemeMode::Dark {
            ButtonVisuals::active(palette)
        } else {
            ButtonVisuals::neutral(palette)
        },
        16.0,
        SMALL_BUTTON_RADIUS,
    )
    .clicked()
    {
        *theme = ThemeMode::Dark;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowChromeAction {
    StartDrag,
    Minimize,
    ToggleMaximize,
    Close,
}

pub(crate) fn render_window_chrome(
    ui: &mut egui::Ui,
    drag_rect: Rect,
    controls_rect: Rect,
    scale: &LayoutScale,
) -> Option<WindowChromeAction> {
    let drag_response = ui.interact(
        drag_rect,
        Id::new("window_drag_strip"),
        Sense::click_and_drag(),
    );

    if drag_response.double_clicked() {
        return Some(WindowChromeAction::ToggleMaximize);
    }

    if drag_response.drag_started_by(egui::PointerButton::Primary) {
        return Some(WindowChromeAction::StartDrag);
    }

    let diameter = controls_rect.height().min(scale.px(18.0));
    let gap = scale.px(10.0);
    let total_width = diameter * 3.0 + gap * 2.0;
    let start_x = controls_rect.right() - total_width;
    let top = controls_rect.center().y - diameter / 2.0;

    let buttons = [
        (
            "window_control_maximize",
            Color32::from_rgb(40, 201, 64),
            WindowChromeAction::Minimize,
        ),
        (
            "window_control_minimize",
            Color32::from_rgb(255, 189, 46),
            WindowChromeAction::ToggleMaximize,
        ),
        (
            "window_control_close",
            Color32::from_rgb(255, 95, 86),
            WindowChromeAction::Close,
        ),
    ];

    for (index, (id, color, action)) in buttons.into_iter().enumerate() {
        let rect = Rect::from_min_size(
            egui::pos2(start_x + index as f32 * (diameter + gap), top),
            egui::vec2(diameter, diameter),
        );
        let response = ui.interact(rect, Id::new(id), Sense::click());
        let fill = if response.hovered() {
            color.gamma_multiply(0.88)
        } else {
            color
        };

        ui.painter()
            .circle_filled(rect.center(), rect.width() / 2.0, fill);

        if response.clicked() {
            return Some(action);
        }
    }

    None
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_log_panel(
    ui: &mut egui::Ui,
    rect: Rect,
    body_rect: Rect,
    button_rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    status_text: &str,
    created_paths: &[PathBuf],
    is_running: bool,
    running_for: Option<Duration>,
) -> bool {
    show_card(
        ui,
        rect,
        scale,
        palette,
        palette.card,
        CARD_RADIUS,
        14.0,
        true,
        |ui| {
            ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
                ui.add_space(scale.px(2.0));
                ui.label(
                    RichText::new("Журнал")
                        .font(FontId::proportional(scale.px(22.0)))
                        .color(palette.text),
                );
            });
        },
    );

    let mut content = String::new();
    if !status_text.trim().is_empty() {
        content.push_str(status_text);
    }
    if is_running {
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str("Выполняется обработка...");
        if let Some(duration) = running_for {
            let total_seconds = duration.as_secs();
            let _ = write!(
                content,
                "\nПрошло: {:02}:{:02}",
                total_seconds / 60,
                total_seconds % 60
            );
        }
    }

    ui.scope_builder(
        UiBuilder::new()
            .max_rect(body_rect)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            let frame = Frame::new()
                .fill(palette.card)
                .stroke(Stroke::new(scale.px(1.0), palette.border))
                .corner_radius(CornerRadius::same(scale.radius(CARD_RADIUS)))
                .inner_margin(Margin::same(scale.margin(12.0)));
            frame.show(ui, |ui| {
                let available_size = ui.available_size();
                ui.set_min_size(available_size);
                if !content.is_empty() {
                    ScrollArea::vertical()
                        .id_salt("log_panel_scroll")
                        .max_height(available_size.y)
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .show(ui, |ui| {
                            ui.set_width(available_size.x);
                            ui.add(
                                TextEdit::multiline(&mut content)
                                    .font(FontId::monospace(scale.px(12.0)))
                                    .desired_width(f32::INFINITY)
                                    .interactive(false)
                                    .frame(Frame::NONE),
                            );
                        });
                } else {
                    ui.label(
                        RichText::new("Лог")
                            .font(FontId::proportional(scale.px(16.0)))
                            .color(palette.muted),
                    );
                }
            });
        },
    );

    let has_paths = !created_paths.is_empty();
    put_button(
        ui,
        button_rect,
        scale,
        "Открыть папку",
        if has_paths {
            ButtonVisuals {
                fill: Color32::from_rgb(0x80, 0xcb, 0x7f),
                text: palette.accent_text,
                stroke: palette.border,
            }
        } else {
            ButtonVisuals::disabled(palette)
        },
        BODY_SIZE,
        SMALL_BUTTON_RADIUS,
    )
    .clicked()
        && has_paths
}
