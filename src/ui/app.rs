use std::any::Any;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use chrono::{Datelike, Local};
use eframe::{
    NativeOptions,
    egui::{self, Frame, Margin, Vec2, ViewportBuilder, ViewportCommand},
};
use once_cell::sync::Lazy;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "linux")]
use std::ffi::{CStr, c_char, c_int, c_uint, c_ulong, c_void};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn},
    UI::WindowsAndMessaging::GetClientRect,
};

use crate::cli::Cli;
use crate::domain::Mest;
use crate::ui::kinds::{ModeKind, ThemeMode};

use super::components::{
    WindowChromeAction, paint_shell, render_brand_card, render_log_panel, render_sidebar,
    render_theme_card, render_window_chrome,
};
use super::screens::{
    batch::BatchScreen, gdi::GdiScreen, gdm::GdmScreen, ppl::PplScreen, ss::SsScreen,
    telemetry::TelemetryScreen, ved::VedScreen,
};
use super::theme::{
    BRAND_CARD_RECT, DESIGN_HEIGHT, DESIGN_WIDTH, LOG_BODY_RECT, LOG_BUTTON_RECT, LOG_CARD_RECT,
    LayoutScale, SHELL_RECT, SIDEBAR_CARD_RECT, THEME_CARD_RECT, WINDOW_CONTROLS_RECT,
    WINDOW_DRAG_RECT, palette,
};

pub fn run(cli: Cli) -> Result<()> {
    let native_options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("ONE BIG SCRIPT")
            .with_decorations(false)
            .with_transparent(window_uses_transparent_viewport())
            .with_inner_size(Vec2::new(DESIGN_WIDTH, DESIGN_HEIGHT))
            .with_min_inner_size(Vec2::new(980.0, 760.0)),
        persist_window: false,
        ..Default::default()
    };

    eframe::run_native(
        "ONE BIG SCRIPT",
        native_options,
        Box::new(|cc| Ok(Box::new(OneBigScriptApp::new(cli, cc)))),
    )
    .map_err(|error| anyhow!(error.to_string()))
}

struct OneBigScriptApp {
    state: AppState,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    native_window: Option<NativeWindowShape>,
}

impl OneBigScriptApp {
    fn new(cli: Cli, cc: &eframe::CreationContext<'_>) -> Self {
        install_roboto_fonts(&cc.egui_ctx);
        Self {
            state: AppState::new(cli),
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            native_window: capture_native_window(cc),
        }
    }

    fn sync_native_window_shape(&mut self, ctx: &egui::Context) {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if let Some(native_window) = &mut self.native_window {
            let is_maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            let native_pixels_per_point =
                ctx.input(|input| input.viewport().native_pixels_per_point.unwrap_or(1.0));
            native_window.apply(25.0, native_pixels_per_point, is_maximized);
        }
    }
}

fn install_roboto_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "roboto".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Roboto.ttf"
        ))),
    );

    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "roboto".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.insert(0, "roboto".to_owned());
    }

    ctx.set_fonts(fonts);
}

impl eframe::App for OneBigScriptApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_native_window_shape(ctx);

        if self.state.is_any_task_running() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if window_uses_transparent_viewport() {
            egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
        } else {
            palette(self.state.theme).matte.to_normalized_gamma_f32()
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ui_palette = palette(self.state.theme);
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(window_background_fill(ui_palette))
                    .inner_margin(Margin::ZERO),
            )
            .show_inside(ui, |ui| {
                let scale = LayoutScale::fit(ui.max_rect());

                paint_shell(ui, scale.rect(SHELL_RECT), &scale, ui_palette);
                let is_maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                if let Some(action) = render_window_chrome(
                    ui,
                    scale.rect(WINDOW_DRAG_RECT),
                    scale.rect(WINDOW_CONTROLS_RECT),
                    &scale,
                ) {
                    match action {
                        WindowChromeAction::StartDrag => {
                            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                        }
                        WindowChromeAction::Minimize => {
                            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                        }
                        WindowChromeAction::ToggleMaximize => {
                            ctx.send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
                        }
                        WindowChromeAction::Close => {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    }
                }
                render_brand_card(
                    ui,
                    scale.rect(BRAND_CARD_RECT),
                    &scale,
                    ui_palette,
                    self.state.screen.ui_label(),
                );

                if let Some(next_screen) = render_sidebar(
                    ui,
                    scale.rect(SIDEBAR_CARD_RECT),
                    &scale,
                    ui_palette,
                    self.state.screen,
                ) {
                    self.state.screen = next_screen;
                }

                render_theme_card(
                    ui,
                    scale.rect(THEME_CARD_RECT),
                    &scale,
                    ui_palette,
                    &mut self.state.theme,
                );

                self.state.render_active_screen(ui, &scale, ui_palette);

                // Close automatically when an autorun task finishes successfully.
                if self.state.should_autoclose() {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }

                let (status_text, created_paths, is_running, running_for) =
                    self.state.active_status_and_paths();
                if render_log_panel(
                    ui,
                    scale.rect(LOG_CARD_RECT),
                    scale.rect(LOG_BODY_RECT),
                    scale.rect(LOG_BUTTON_RECT),
                    &scale,
                    ui_palette,
                    status_text,
                    created_paths,
                    is_running,
                    running_for,
                ) {
                    let _ = open_result_dirs(created_paths);
                }
            });
    }
}

fn window_background_fill(palette: super::theme::Palette) -> egui::Color32 {
    if window_uses_transparent_viewport() {
        egui::Color32::TRANSPARENT
    } else {
        palette.matte
    }
}

const fn window_uses_transparent_viewport() -> bool {
    false
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct NativeWindowShape {
    hwnd: HWND,
    last_client_size: Option<(i32, i32)>,
    last_radius_px: Option<i32>,
    last_maximized: Option<bool>,
}

#[cfg(target_os = "windows")]
impl NativeWindowShape {
    fn apply(&mut self, radius_points: f32, native_pixels_per_point: f32, is_maximized: bool) {
        let Some((width, height)) = client_size(self.hwnd) else {
            return;
        };

        let radius_px = (radius_points * native_pixels_per_point).round() as i32;
        if self.last_client_size == Some((width, height))
            && self.last_radius_px == Some(radius_px)
            && self.last_maximized == Some(is_maximized)
        {
            return;
        }

        // Keep maximized windows rectangular so they fit the monitor bounds cleanly.
        if is_maximized {
            unsafe {
                SetWindowRgn(self.hwnd, std::ptr::null_mut(), 1);
            }
        } else {
            apply_rounded_region(self.hwnd, width, height, radius_px.max(1));
        }

        self.last_client_size = Some((width, height));
        self.last_radius_px = Some(radius_px.max(1));
        self.last_maximized = Some(is_maximized);
    }
}

#[cfg(target_os = "windows")]
fn capture_native_window(cc: &eframe::CreationContext<'_>) -> Option<NativeWindowShape> {
    let raw_window = cc.window_handle().ok()?.as_raw();
    let hwnd = match raw_window {
        RawWindowHandle::Win32(handle) => handle.hwnd.get() as HWND,
        _ => return None,
    };

    Some(NativeWindowShape {
        hwnd,
        last_client_size: None,
        last_radius_px: None,
        last_maximized: None,
    })
}

#[cfg(target_os = "windows")]
fn client_size(hwnd: HWND) -> Option<(i32, i32)> {
    let mut rect = RECT::default();
    let ok = unsafe { GetClientRect(hwnd, &mut rect) };
    if ok == 0 {
        return None;
    }

    Some((rect.right - rect.left, rect.bottom - rect.top))
}

#[cfg(target_os = "windows")]
fn apply_rounded_region(hwnd: HWND, width: i32, height: i32, radius_px: i32) {
    let ellipse = radius_px.saturating_mul(2);
    let region = unsafe { CreateRoundRectRgn(0, 0, width + 1, height + 1, ellipse, ellipse) };
    if region.is_null() {
        return;
    }

    let applied = unsafe { SetWindowRgn(hwnd, region, 1) };
    if applied == 0 {
        unsafe {
            DeleteObject(region.cast());
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct NativeWindowShape {
    api: &'static LinuxX11Api,
    display: *mut c_void,
    window: c_ulong,
    last_client_size: Option<(i32, i32)>,
    last_radius_px: Option<i32>,
    last_maximized: Option<bool>,
}

#[cfg(target_os = "linux")]
impl Drop for NativeWindowShape {
    fn drop(&mut self) {
        unsafe {
            (self.api.x_close_display)(self.display);
        }
    }
}

#[cfg(target_os = "linux")]
impl NativeWindowShape {
    fn apply(&mut self, radius_points: f32, native_pixels_per_point: f32, is_maximized: bool) {
        let Some((width, height)) = client_size(self.api, self.display, self.window) else {
            return;
        };

        let radius_px = (radius_points * native_pixels_per_point).round() as i32;
        let applied_radius = if is_maximized { 0 } else { radius_px.max(1) };
        if self.last_client_size == Some((width, height))
            && self.last_radius_px == Some(applied_radius)
            && self.last_maximized == Some(is_maximized)
        {
            return;
        }

        apply_rounded_region(
            self.api,
            self.display,
            self.window,
            width,
            height,
            applied_radius,
        );

        self.last_client_size = Some((width, height));
        self.last_radius_px = Some(applied_radius);
        self.last_maximized = Some(is_maximized);
    }
}

#[cfg(target_os = "linux")]
fn capture_native_window(cc: &eframe::CreationContext<'_>) -> Option<NativeWindowShape> {
    let api = LINUX_X11_API.as_ref()?;
    let raw_window = cc.window_handle().ok()?.as_raw();
    let window = match raw_window {
        RawWindowHandle::Xlib(handle) => handle.window,
        RawWindowHandle::Xcb(handle) => handle.window.get() as c_ulong,
        _ => return None,
    };

    let display = unsafe { (api.x_open_display)(std::ptr::null::<c_char>()) };
    if display.is_null() {
        return None;
    }

    Some(NativeWindowShape {
        api,
        display,
        window,
        last_client_size: None,
        last_radius_px: None,
        last_maximized: None,
    })
}

#[cfg(target_os = "linux")]
fn client_size(api: &LinuxX11Api, display: *mut c_void, window: c_ulong) -> Option<(i32, i32)> {
    let mut root_window = 0 as c_ulong;
    let mut x = 0;
    let mut y = 0;
    let mut width = 0 as c_uint;
    let mut height = 0 as c_uint;
    let mut border_width = 0 as c_uint;
    let mut depth = 0 as c_uint;

    let ok = unsafe {
        (api.x_get_geometry)(
            display,
            window,
            &mut root_window,
            &mut x,
            &mut y,
            &mut width,
            &mut height,
            &mut border_width,
            &mut depth,
        )
    };
    if ok == 0 || width == 0 || height == 0 {
        return None;
    }

    Some((width as i32, height as i32))
}

#[cfg(target_os = "linux")]
fn apply_rounded_region(
    api: &LinuxX11Api,
    display: *mut c_void,
    window: c_ulong,
    width: i32,
    height: i32,
    radius_px: i32,
) {
    if width <= 0 || height <= 0 {
        return;
    }

    let region = unsafe { (api.x_create_region)() };
    if region.is_null() {
        return;
    }

    for y in 0..height {
        let inset = horizontal_inset_for_row(height, radius_px, y);
        let row_width = (width - inset * 2).max(1);
        union_rect(api, region, inset, y, row_width, 1);
    }

    unsafe {
        (api.x_shape_combine_region)(display, window, SHAPE_BOUNDING, 0, 0, region, SHAPE_SET);
        (api.x_destroy_region)(region);
        (api.x_sync)(display, 0);
    }
}

#[cfg(target_os = "linux")]
fn horizontal_inset_for_row(height: i32, radius_px: i32, y: i32) -> i32 {
    if radius_px <= 0 {
        return 0;
    }

    let top_distance = y;
    let bottom_distance = height - 1 - y;
    let distance_from_corner = if top_distance < radius_px {
        top_distance
    } else if bottom_distance < radius_px {
        bottom_distance
    } else {
        return 0;
    };

    rounded_corner_inset(radius_px, distance_from_corner)
}

#[cfg(target_os = "linux")]
fn rounded_corner_inset(radius_px: i32, distance_from_corner: i32) -> i32 {
    let radius = radius_px as f64;
    let center = radius - 0.5;
    let y = distance_from_corner as f64;
    let dy = center - y;
    let dx = (radius * radius - dy * dy).max(0.0).sqrt();
    (center - dx).ceil().max(0.0) as i32
}

#[cfg(target_os = "linux")]
fn union_rect(api: &LinuxX11Api, region: Region, x: i32, y: i32, width: i32, height: i32) {
    let rect = XRectangle {
        x: x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        y: y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        width: width.clamp(0, u16::MAX as i32) as u16,
        height: height.clamp(0, u16::MAX as i32) as u16,
    };

    unsafe {
        (api.x_union_rect_with_region)(&rect, region, region);
    }
}

#[cfg(target_os = "linux")]
type Region = *mut c_void;

#[cfg(target_os = "linux")]
#[repr(C)]
struct XRectangle {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

#[cfg(target_os = "linux")]
const SHAPE_BOUNDING: c_int = 0;

#[cfg(target_os = "linux")]
const SHAPE_SET: c_int = 0;

#[cfg(target_os = "linux")]
type XOpenDisplayFn = unsafe extern "C" fn(display_name: *const c_char) -> *mut c_void;
#[cfg(target_os = "linux")]
type XCloseDisplayFn = unsafe extern "C" fn(display: *mut c_void) -> c_int;
#[cfg(target_os = "linux")]
type XCreateRegionFn = unsafe extern "C" fn() -> Region;
#[cfg(target_os = "linux")]
type XDestroyRegionFn = unsafe extern "C" fn(region: Region) -> c_int;
#[cfg(target_os = "linux")]
type XUnionRectWithRegionFn =
    unsafe extern "C" fn(rectangle: *const XRectangle, src_region: Region, dest_region: Region);
#[cfg(target_os = "linux")]
type XGetGeometryFn = unsafe extern "C" fn(
    display: *mut c_void,
    drawable: c_ulong,
    root_return: *mut c_ulong,
    x_return: *mut c_int,
    y_return: *mut c_int,
    width_return: *mut c_uint,
    height_return: *mut c_uint,
    border_width_return: *mut c_uint,
    depth_return: *mut c_uint,
) -> c_int;
#[cfg(target_os = "linux")]
type XSyncFn = unsafe extern "C" fn(display: *mut c_void, discard: c_int) -> c_int;
#[cfg(target_os = "linux")]
type XShapeCombineRegionFn = unsafe extern "C" fn(
    display: *mut c_void,
    dest: c_ulong,
    dest_kind: c_int,
    x_off: c_int,
    y_off: c_int,
    region: Region,
    op: c_int,
);

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxX11Api {
    _lib_x11: *mut c_void,
    _lib_xext: *mut c_void,
    x_open_display: XOpenDisplayFn,
    x_close_display: XCloseDisplayFn,
    x_create_region: XCreateRegionFn,
    x_destroy_region: XDestroyRegionFn,
    x_union_rect_with_region: XUnionRectWithRegionFn,
    x_get_geometry: XGetGeometryFn,
    x_sync: XSyncFn,
    x_shape_combine_region: XShapeCombineRegionFn,
}

#[cfg(target_os = "linux")]
unsafe impl Send for LinuxX11Api {}
#[cfg(target_os = "linux")]
unsafe impl Sync for LinuxX11Api {}

#[cfg(target_os = "linux")]
static LINUX_X11_API: Lazy<Option<LinuxX11Api>> = Lazy::new(|| unsafe {
    let lib_x11 = open_first(&[b"libX11.so.6\0", b"libX11.so\0"])?;
    let lib_xext = open_first(&[b"libXext.so.6\0", b"libXext.so\0"])?;

    Some(LinuxX11Api {
        _lib_x11: lib_x11,
        _lib_xext: lib_xext,
        x_open_display: load_symbol(lib_x11, cstr(b"XOpenDisplay\0"))?,
        x_close_display: load_symbol(lib_x11, cstr(b"XCloseDisplay\0"))?,
        x_create_region: load_symbol(lib_x11, cstr(b"XCreateRegion\0"))?,
        x_destroy_region: load_symbol(lib_x11, cstr(b"XDestroyRegion\0"))?,
        x_union_rect_with_region: load_symbol(lib_x11, cstr(b"XUnionRectWithRegion\0"))?,
        x_get_geometry: load_symbol(lib_x11, cstr(b"XGetGeometry\0"))?,
        x_sync: load_symbol(lib_x11, cstr(b"XSync\0"))?,
        x_shape_combine_region: load_symbol(lib_xext, cstr(b"XShapeCombineRegion\0"))?,
    })
});

#[cfg(target_os = "linux")]
fn cstr(bytes: &'static [u8]) -> &'static CStr {
    CStr::from_bytes_with_nul(bytes).expect("static C string must be NUL-terminated")
}

#[cfg(target_os = "linux")]
unsafe fn open_first(candidates: &[&[u8]]) -> Option<*mut c_void> {
    for candidate in candidates {
        // SAFETY: `candidate` is a NUL-terminated library name borrowed for this call.
        let handle = unsafe { dlopen(candidate.as_ptr().cast::<c_char>(), RTLD_NOW | RTLD_LOCAL) };
        if !handle.is_null() {
            return Some(handle);
        }
    }
    None
}

#[cfg(target_os = "linux")]
unsafe fn load_symbol<T: Copy>(library: *mut c_void, symbol: &'static CStr) -> Option<T> {
    // SAFETY: `library` is expected to be a live handle returned by `dlopen`,
    // and `symbol` is a valid NUL-terminated symbol name.
    let ptr = unsafe { dlsym(library, symbol.as_ptr()) };
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the caller picks `T` to match the requested symbol's function pointer type.
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&ptr) })
    }
}

#[cfg(target_os = "linux")]
const RTLD_LOCAL: c_int = 0;
#[cfg(target_os = "linux")]
const RTLD_NOW: c_int = 2;

#[cfg(target_os = "linux")]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

struct AppState {
    autorun_mode: bool,
    theme: ThemeMode,
    screen: ModeKind,
    batch: BatchScreen,
    gdi: GdiScreen,
    gdm: GdmScreen,
    ppl: PplScreen,
    ss: SsScreen,
    telemetry: TelemetryScreen,
    ved: VedScreen,
}

impl AppState {
    fn new(cli: Cli) -> Self {
        let autorun_mode = cli.is_autorun();
        let theme = cli.theme;
        let screen = cli.initial_mode();
        let gdi = if cli.gdi {
            GdiScreen::from_cli(&cli)
        } else {
            GdiScreen::default()
        };
        let ppl = if cli.ppl {
            PplScreen::from_cli(&cli)
        } else {
            PplScreen::default()
        };
        let ss = if cli.ss {
            SsScreen::from_cli(&cli)
        } else {
            SsScreen::default()
        };
        let ved = if cli.ved {
            VedScreen::from_cli(&cli)
        } else {
            VedScreen::default()
        };
        let gdm = if cli.gdm {
            GdmScreen::from_cli(&cli)
        } else {
            GdmScreen::default()
        };
        Self {
            autorun_mode,
            theme,
            screen,
            batch: BatchScreen::default(),
            gdi,
            gdm,
            ppl,
            ss,
            telemetry: TelemetryScreen::default(),
            ved,
        }
    }

    /// Returns true when an autorun task has completed successfully —
    /// the window should close automatically in that case.
    fn should_autoclose(&self) -> bool {
        if !self.autorun_mode {
            return false;
        }
        let (status, is_running) = match self.screen {
            ModeKind::Gdi => (self.gdi.status_and_paths().0, self.gdi.is_running()),
            ModeKind::Ss => (self.ss.status_and_paths().0, self.ss.is_running()),
            ModeKind::Ppl => (self.ppl.status_and_paths().0, self.ppl.is_running()),
            ModeKind::Ved => (self.ved.status_and_paths().0, self.ved.is_running()),
            ModeKind::Gdm => (self.gdm.status_and_paths().0, self.gdm.is_running()),
            _ => return false,
        };
        // Close only when: task finished, no error, and status is non-empty
        // (empty = not started yet).
        !is_running && !status.is_empty() && !status.starts_with("Ошибка")
    }

    fn render_active_screen(
        &mut self,
        ui: &mut egui::Ui,
        scale: &LayoutScale,
        ui_palette: super::theme::Palette,
    ) {
        match self.screen {
            ModeKind::Home | ModeKind::Batch => self.batch.render(ui, scale, ui_palette),
            ModeKind::Gdi => self.gdi.render(ui, scale, ui_palette),
            ModeKind::Gdm => self.gdm.render(ui, scale, ui_palette),
            ModeKind::Ppl => self.ppl.render(ui, scale, ui_palette),
            ModeKind::Ss => self.ss.render(ui, scale, ui_palette),
            ModeKind::Telemetry => self.telemetry.render(ui, scale, ui_palette),
            ModeKind::Ved => self.ved.render(ui, scale, ui_palette),
        }
    }

    fn active_status_and_paths(&self) -> (&str, &[PathBuf], bool, Option<Duration>) {
        match self.screen {
            ModeKind::Home | ModeKind::Batch => (
                self.batch.status_and_paths().0,
                self.batch.status_and_paths().1,
                self.batch.is_running(),
                self.batch.running_for(),
            ),
            ModeKind::Gdi => (
                self.gdi.status_and_paths().0,
                self.gdi.status_and_paths().1,
                self.gdi.is_running(),
                self.gdi.running_for(),
            ),
            ModeKind::Gdm => (
                self.gdm.status_and_paths().0,
                self.gdm.status_and_paths().1,
                self.gdm.is_running(),
                self.gdm.running_for(),
            ),
            ModeKind::Ppl => (
                self.ppl.status_and_paths().0,
                self.ppl.status_and_paths().1,
                self.ppl.is_running(),
                self.ppl.running_for(),
            ),
            ModeKind::Ss => (
                self.ss.status_and_paths().0,
                self.ss.status_and_paths().1,
                self.ss.is_running(),
                self.ss.running_for(),
            ),
            ModeKind::Telemetry => (
                self.telemetry.status_and_paths().0,
                self.telemetry.status_and_paths().1,
                self.telemetry.is_running(),
                self.telemetry.running_for(),
            ),
            ModeKind::Ved => (
                self.ved.status_and_paths().0,
                self.ved.status_and_paths().1,
                self.ved.is_running(),
                self.ved.running_for(),
            ),
        }
    }

    fn is_any_task_running(&self) -> bool {
        self.batch.is_running()
            || self.gdi.is_running()
            || self.gdm.is_running()
            || self.ppl.is_running()
            || self.ss.is_running()
            || self.telemetry.is_running()
            || self.ved.is_running()
    }
}

pub(crate) struct TaskState<T> {
    pub(crate) running: bool,
    status_text: String,
    created_paths: Vec<PathBuf>,
    started_at: Option<Instant>,
    rx: Option<Receiver<Result<T>>>,
}

impl<T> Default for TaskState<T> {
    fn default() -> Self {
        Self {
            running: false,
            status_text: String::new(),
            created_paths: Vec::new(),
            started_at: None,
            rx: None,
        }
    }
}

impl<T> TaskState<T> {
    pub(crate) fn start(&mut self, rx: Receiver<Result<T>>, status: impl Into<String>) {
        self.running = true;
        self.status_text = status.into();
        self.created_paths.clear();
        self.started_at = Some(Instant::now());
        self.rx = Some(rx);
    }

    pub(crate) fn update_with<F, D>(&mut self, on_success: F, on_disconnected: D)
    where
        F: FnOnce(T) -> (String, Vec<PathBuf>),
        D: FnOnce() -> String,
    {
        let Some(rx) = &self.rx else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(value)) => {
                let (status_text, created_paths) = on_success(value);
                self.running = false;
                self.status_text = status_text;
                self.created_paths = created_paths;
                self.started_at = None;
                self.rx = None;
            }
            Ok(Err(error)) => {
                self.running = false;
                self.status_text = format!("Ошибка: {error:#}");
                self.created_paths.clear();
                self.started_at = None;
                self.rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.running = false;
                self.status_text = on_disconnected();
                self.created_paths.clear();
                self.started_at = None;
                self.rx = None;
            }
        }
    }

    pub(crate) fn status_and_paths(&self) -> (&str, &[PathBuf]) {
        (&self.status_text, &self.created_paths)
    }

    pub(crate) fn running_for(&self) -> Option<Duration> {
        self.started_at.map(|started_at| started_at.elapsed())
    }
}

pub(crate) fn default_selected_mests() -> BTreeSet<Mest> {
    BTreeSet::new()
}

pub(crate) fn default_year() -> i32 {
    Local::now().year()
}

pub(crate) fn default_month() -> u32 {
    1
}

pub(crate) fn default_day() -> u32 {
    1
}

static YEAR_OPTIONS: Lazy<Vec<i32>> = Lazy::new(|| (1971..=default_year()).rev().collect());
const MONTH_OPTIONS: [u32; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const DAY_OPTIONS: [u32; 31] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31,
];

pub(crate) fn year_options() -> &'static [i32] {
    YEAR_OPTIONS.as_slice()
}

pub(crate) fn month_options() -> &'static [u32] {
    &MONTH_OPTIONS
}

pub(crate) fn day_options() -> &'static [u32] {
    &DAY_OPTIONS
}

pub(crate) fn finish_with_paths(paths: Vec<PathBuf>) -> (String, Vec<PathBuf>) {
    (format_paths(&paths), paths)
}

pub(crate) fn format_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        "Готово: итоговые файлы не были созданы.".to_string()
    } else {
        format!(
            "Готово. Создано файлов: {}\n{}",
            paths.len(),
            crate::paths::format_candidate_paths(paths)
        )
    }
}

pub(crate) fn result_dirs(paths: &[PathBuf]) -> Vec<PathBuf> {
    crate::paths::preferred_result_dirs(paths)
}

pub(crate) fn open_result_dirs(paths: &[PathBuf]) -> Result<()> {
    let dirs = result_dirs(paths);
    if dirs.is_empty() {
        return Err(anyhow!("Нет папок с результатами для открытия."));
    }

    for dir in dirs {
        open_directory(&dir)?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn open_directory(dir: &Path) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if !dir.is_dir() {
        return Err(anyhow!("Папка результата не найдена: {}", dir.display()));
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let operation = wide_null(OsStr::new("open"));
    let path = wide_null(dir.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };

    if result as isize <= 32 {
        return Err(anyhow!(
            "Не удалось открыть папку {}: ShellExecuteW вернул код {}",
            dir.display(),
            result as isize
        ));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn open_directory(dir: &Path) -> Result<()> {
    use anyhow::Context;

    if !dir.is_dir() {
        return Err(anyhow!("Папка результата не найдена: {}", dir.display()));
    }

    std::process::Command::new("xdg-open")
        .arg(dir)
        .spawn()
        .with_context(|| format!("Не удалось открыть {}", dir.display()))?;

    Ok(())
}

pub(crate) fn spawn_result_task<T, F>(tx: Sender<Result<T>>, task_name: &'static str, task: F)
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    thread::spawn(move || {
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(task)) {
            Ok(result) => result,
            Err(payload) => Err(anyhow!(
                "Паника в фоновой задаче {task_name}: {}",
                panic_payload_message(payload)
            )),
        };

        let _ = tx.send(result);
    });
}

pub(crate) fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_string(),
            Err(_) => "неизвестная причина".to_string(),
        },
    }
}
