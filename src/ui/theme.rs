use egui::{Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};

use crate::ui::kinds::ThemeMode;

pub const DESIGN_WIDTH: f32 = 1199.0;
pub const DESIGN_HEIGHT: f32 = 897.0;
pub const DESIGN_OFFSET_X: f32 = 88.0;
pub const DESIGN_OFFSET_Y: f32 = 73.0;

pub const SHELL_RECT: RectSpec = RectSpec::new(
    DESIGN_OFFSET_X,
    DESIGN_OFFSET_Y,
    DESIGN_WIDTH,
    DESIGN_HEIGHT,
);
pub const BRAND_CARD_RECT: RectSpec = RectSpec::new(104.0, 88.0, 226.0, 141.0);
pub const SIDEBAR_CARD_RECT: RectSpec = RectSpec::new(104.0, 247.0, 227.0, 549.0);
pub const THEME_CARD_RECT: RectSpec = RectSpec::new(104.0, 815.0, 226.0, 141.0);
pub const WINDOW_DRAG_RECT: RectSpec = RectSpec::new(1049.0, 84.0, 136.0, 22.0);
pub const WINDOW_CONTROLS_RECT: RectSpec = RectSpec::new(1188.0, 82.0, 74.0, 22.0);
pub const LOG_CARD_RECT: RectSpec = RectSpec::new(1047.0, 112.0, 225.0, 843.0);
pub const LOG_BODY_RECT: RectSpec = RectSpec::new(1061.0, 158.0, 197.0, 714.0);
pub const LOG_BUTTON_RECT: RectSpec = RectSpec::new(1060.0, 910.0, 200.0, 34.0);

pub const OUTER_CARD_RADIUS: f32 = 28.0;
pub const CARD_RADIUS: f32 = 22.0;
pub const SECTION_RADIUS: f32 = 20.0;
pub const BUTTON_RADIUS: f32 = 18.0;
pub const SMALL_BUTTON_RADIUS: f32 = 12.0;

pub const CARD_BORDER: f32 = 1.25;
pub const TITLE_SIZE: f32 = 14.0;
pub const BODY_SIZE: f32 = 13.0;
pub const BIG_TEXT_SIZE: f32 = 17.0;
pub const HERO_SIZE: f32 = 18.0;

#[derive(Debug, Clone, Copy)]
pub struct RectSpec {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectSpec {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutScale {
    pub scale_x: f32,
    pub scale_y: f32,
    pub unit: f32,
    pub origin: Pos2,
}

impl LayoutScale {
    pub fn fit(bounds: Rect) -> Self {
        let scale_x = (bounds.width() / DESIGN_WIDTH).max(0.1);
        let scale_y = (bounds.height() / DESIGN_HEIGHT).max(0.1);
        let unit = scale_x.min(scale_y);
        let origin = bounds.min;
        Self {
            scale_x,
            scale_y,
            unit,
            origin,
        }
    }

    pub fn rect(&self, spec: RectSpec) -> Rect {
        Rect::from_min_size(
            self.point(spec.x, spec.y),
            self.size(spec.width, spec.height),
        )
    }

    pub fn point(&self, x: f32, y: f32) -> Pos2 {
        pos2(
            self.origin.x + (x - DESIGN_OFFSET_X) * self.scale_x,
            self.origin.y + (y - DESIGN_OFFSET_Y) * self.scale_y,
        )
    }

    pub fn size(&self, width: f32, height: f32) -> Vec2 {
        vec2(width * self.scale_x, height * self.scale_y)
    }

    pub fn px(&self, value: f32) -> f32 {
        value * self.unit
    }

    pub fn radius(&self, value: f32) -> u8 {
        self.px(value).round().clamp(0.0, u8::MAX as f32) as u8
    }

    pub fn margin(&self, value: f32) -> i8 {
        self.px(value).round().clamp(0.0, i8::MAX as f32) as i8
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub matte: Color32,
    pub shell: Color32,
    pub card: Color32,
    pub section: Color32,
    pub neutral_button: Color32,
    pub neutral_button_alt: Color32,
    pub accent: Color32,
    pub accent_alt: Color32,
    pub accent_text: Color32,
    pub danger: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_soft: Color32,
    pub muted: Color32,
    pub shadow: Color32,
}

impl Palette {
    pub fn stroke(self) -> Stroke {
        Stroke::new(CARD_BORDER, self.border)
    }
}

pub fn palette(theme: ThemeMode) -> Palette {
    match theme {
        ThemeMode::Light => Palette {
            matte: Color32::from_rgb(255, 255, 255),
            shell: Color32::from_rgb(235, 235, 235),
            card: Color32::from_rgb(255, 255, 255),
            section: Color32::from_rgb(231, 231, 231),
            neutral_button: Color32::from_rgb(209, 209, 209),
            neutral_button_alt: Color32::from_rgb(215, 215, 215),
            accent: Color32::from_rgb(112, 207, 196),
            accent_alt: Color32::from_rgb(121, 216, 206),
            accent_text: Color32::from_rgb(21, 21, 21),
            danger: Color32::from_rgb(203, 127, 128),
            border: Color32::from_rgb(38, 38, 38),
            text: Color32::from_rgb(21, 21, 21),
            text_soft: Color32::from_rgb(45, 45, 45),
            muted: Color32::from_rgb(90, 90, 90),
            shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 56),
        },
        ThemeMode::Dark => Palette {
            matte: Color32::from_rgb(32, 35, 40),
            shell: Color32::from_rgb(39, 42, 48),
            card: Color32::from_rgb(52, 56, 64),
            section: Color32::from_rgb(43, 47, 55),
            neutral_button: Color32::from_rgb(78, 82, 90),
            neutral_button_alt: Color32::from_rgb(69, 73, 80),
            accent: Color32::from_rgb(104, 193, 183),
            accent_alt: Color32::from_rgb(118, 206, 196),
            accent_text: Color32::from_rgb(26, 32, 36),
            danger: Color32::from_rgb(164, 94, 95),
            border: Color32::from_rgb(178, 184, 194),
            text: Color32::from_rgb(244, 244, 244),
            text_soft: Color32::from_rgb(225, 225, 225),
            muted: Color32::from_rgb(192, 192, 192),
            shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 90),
        },
    }
}
