use std::hash::Hash;

use egui::{
    Align, Align2, Button, Color32, ComboBox, CornerRadius, FontId, Layout, Rect, Response,
    RichText, Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::ui::theme::{LayoutScale, Palette};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ButtonVisuals {
    pub fill: Color32,
    pub text: Color32,
    pub stroke: Color32,
}

impl ButtonVisuals {
    pub fn neutral(palette: Palette) -> Self {
        Self {
            fill: palette.neutral_button,
            text: palette.text,
            stroke: palette.border,
        }
    }

    pub fn active(palette: Palette) -> Self {
        Self {
            fill: palette.accent,
            text: palette.accent_text,
            stroke: palette.border,
        }
    }

    pub fn danger(palette: Palette) -> Self {
        Self {
            fill: palette.danger,
            text: palette.text,
            stroke: palette.border,
        }
    }

    pub fn disabled(palette: Palette) -> Self {
        Self {
            fill: palette.neutral_button_alt,
            text: palette.muted,
            stroke: palette.border,
        }
    }
}

pub(crate) fn put_button(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    label: &str,
    visuals: ButtonVisuals,
    font_size: f32,
    radius: f32,
) -> Response {
    put_button_rich(
        ui,
        rect,
        scale,
        RichText::new(label),
        visuals,
        font_size,
        radius,
    )
}

pub(crate) fn put_button_rich(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    label: RichText,
    visuals: ButtonVisuals,
    font_size: f32,
    radius: f32,
) -> Response {
    ui.put(
        rect,
        Button::new(
            label
                .font(FontId::proportional(scale.px(font_size)))
                .color(visuals.text),
        )
        .fill(visuals.fill)
        .stroke(Stroke::new(scale.px(1.0), visuals.stroke))
        .corner_radius(CornerRadius::same(scale.radius(radius))),
    )
}

pub(crate) fn put_combo_box(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    id_source: impl Hash,
    selected_text: impl Into<String>,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Response {
    let selected_text = selected_text.into();
    let corner_radius = CornerRadius::same(scale.radius((rect.height() / 2.0).max(10.0)));
    let stroke = Stroke::new(scale.px(1.0), palette.border.gamma_multiply(0.72));
    let inactive_fill = palette.section;
    let hovered_fill = palette.neutral_button_alt;
    let active_fill = palette.card;
    let text_stroke = Stroke::new(scale.px(1.0), palette.text);
    let icon_stroke_width = scale.px(1.8);

    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Min)),
        |ui| {
            ui.set_min_size(rect.size());
            ui.spacing_mut().interact_size = rect.size();
            ui.spacing_mut().button_padding =
                vec2(scale.px(10.0), (rect.height() * 0.14).max(scale.px(2.0)));

            let visuals = ui.visuals_mut();
            visuals.override_text_color = Some(palette.text);
            visuals.selection.bg_fill = palette.accent;
            visuals.selection.stroke = Stroke::new(scale.px(1.0), palette.border);
            visuals.widgets.inactive.bg_fill = inactive_fill;
            visuals.widgets.inactive.weak_bg_fill = inactive_fill;
            visuals.widgets.inactive.bg_stroke = stroke;
            visuals.widgets.inactive.fg_stroke = text_stroke;
            visuals.widgets.inactive.corner_radius = corner_radius;
            visuals.widgets.inactive.expansion = 0.0;
            visuals.widgets.hovered.bg_fill = hovered_fill;
            visuals.widgets.hovered.weak_bg_fill = hovered_fill;
            visuals.widgets.hovered.bg_stroke = stroke;
            visuals.widgets.hovered.fg_stroke = text_stroke;
            visuals.widgets.hovered.corner_radius = corner_radius;
            visuals.widgets.hovered.expansion = 0.0;
            visuals.widgets.active.bg_fill = active_fill;
            visuals.widgets.active.weak_bg_fill = active_fill;
            visuals.widgets.active.bg_stroke = stroke;
            visuals.widgets.active.fg_stroke = text_stroke;
            visuals.widgets.active.corner_radius = corner_radius;
            visuals.widgets.active.expansion = 0.0;
            visuals.widgets.open.bg_fill = active_fill;
            visuals.widgets.open.weak_bg_fill = active_fill;
            visuals.widgets.open.bg_stroke = stroke;
            visuals.widgets.open.fg_stroke = text_stroke;
            visuals.widgets.open.corner_radius = corner_radius;
            visuals.widgets.open.expansion = 0.0;

            ComboBox::from_id_salt(id_source)
                .selected_text(
                    RichText::new(selected_text)
                        .font(FontId::proportional(scale.px(font_size_for_rect(rect))))
                        .color(palette.text),
                )
                .width(rect.width())
                .height(rect.height())
                .icon(move |ui, rect, visuals, is_open| {
                    paint_combo_chevron(ui, rect, visuals, is_open, icon_stroke_width);
                })
                .show_ui(ui, add_contents)
                .response
        },
    )
    .inner
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn put_scrollable_combo_box<T, F>(
    ui: &mut egui::Ui,
    rect: Rect,
    scale: &LayoutScale,
    palette: Palette,
    id_source: impl Hash,
    value: &mut T,
    options: &[T],
    label_for: F,
) -> Response
where
    T: Copy + PartialEq,
    F: Fn(T) -> String + Copy,
{
    let response = put_combo_box(
        ui,
        rect,
        scale,
        palette,
        id_source,
        label_for(*value),
        |ui| {
            for option in options {
                ui.selectable_value(value, *option, label_for(*option));
            }
        },
    );
    apply_combo_box_scroll(ui, &response, value, options);
    response
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_text(
    ui: &egui::Ui,
    scale: &LayoutScale,
    x: f32,
    y: f32,
    align: Align2,
    text: &str,
    font_size: f32,
    color: Color32,
) {
    ui.painter().text(
        scale.point(x, y),
        align,
        text,
        FontId::proportional(scale.px(font_size)),
        color,
    );
}

fn font_size_for_rect(rect: Rect) -> f32 {
    (rect.height() / 2.2).max(12.0)
}

fn apply_combo_box_scroll<T>(
    ui: &egui::Ui,
    response: &Response,
    value: &mut T,
    options: &[T],
) -> bool
where
    T: Copy + PartialEq,
{
    if !response.hovered() || options.len() < 2 {
        return false;
    }

    let scroll_direction = ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel {
                    delta, modifiers, ..
                } if !modifiers.ctrl && !modifiers.command && delta.y != 0.0 => Some(delta.y),
                _ => None,
            })
            .next_back()
            .map(f32::signum)
            .unwrap_or(0.0)
    });

    if scroll_direction == 0.0 {
        return false;
    }

    let Some(index) = options.iter().position(|option| option == value) else {
        return false;
    };

    let next_index = if scroll_direction > 0.0 {
        index.saturating_sub(1)
    } else {
        (index + 1).min(options.len() - 1)
    };

    if next_index == index {
        return false;
    }

    *value = options[next_index];
    true
}

fn paint_combo_chevron(
    ui: &Ui,
    rect: Rect,
    visuals: &egui::style::WidgetVisuals,
    is_open: bool,
    stroke_width: f32,
) {
    let center = rect.center();
    let half_width = rect.width() * 0.18;
    let half_height = rect.height() * 0.13;
    let direction = if is_open { -1.0 } else { 1.0 };

    ui.painter().line_segment(
        [
            pos2(center.x - half_width, center.y - half_height * direction),
            pos2(center.x, center.y + half_height * direction),
        ],
        Stroke::new(stroke_width, visuals.fg_stroke.color.gamma_multiply(0.9)),
    );
    ui.painter().line_segment(
        [
            pos2(center.x, center.y + half_height * direction),
            pos2(center.x + half_width, center.y - half_height * direction),
        ],
        Stroke::new(stroke_width, visuals.fg_stroke.color.gamma_multiply(0.9)),
    );
}
