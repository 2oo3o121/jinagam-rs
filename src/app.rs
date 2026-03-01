use std::env;
use std::fs;
use std::path::PathBuf;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::{
    ChooseColorW, CHOOSECOLORW, CC_ANYCOLOR, CC_FULLOPEN, CC_RGBINIT,
};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, LoadCursorW, PostQuitMessage, RegisterClassW, SetTimer,
    SetWindowLongPtrW, TranslateMessage, CREATESTRUCTW, CW_USEDEFAULT, GWLP_USERDATA, HCURSOR, HMENU,
    IDC_ARROW, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_NCCREATE,
    WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use crate::monitor_cache::MonitorCache;
use crate::overlay_window::{
    performance_for_mode, OverlayOptimizationMode, OverlayStyle, OverlayWindow,
};
use crate::tray::{
    TrayIcon, TrayMenuState, TrayOptimizationMode, ID_COLOR_PICKER, ID_DURATION_LONG,
    ID_DURATION_NORMAL, ID_DURATION_SHORT, ID_EXIT, ID_OPTIMIZE_EFFICIENT, ID_OPTIMIZE_SMOOTH,
    ID_RELOAD_MONITORS, ID_SPAN_FULL, ID_SPAN_SEGMENT, ID_TOGGLE_ENABLED, ID_WIDTH_NORMAL,
    ID_WIDTH_THICK, ID_WIDTH_THIN,
};

const HIDDEN_CLASS: PCWSTR = w!("jinagam_rs_hidden");
const POLL_TIMER_ID: usize = 1;
const POLL_INTERVAL_MS: u32 = 8;
const SPAN_PAD: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundarySpanMode {
    CrossingSegment,
    FullBoundary,
}

#[derive(Clone, Copy)]
struct Settings {
    enabled: bool,
    overlay: OverlayStyle,
    span_mode: BoundarySpanMode,
    optimization: OverlayOptimizationMode,
}

pub struct App {
    instance: HINSTANCE,
    hidden_hwnd: HWND,
    monitor_cache: MonitorCache,
    tray: TrayIcon,
    overlay: OverlayWindow,
    custom_colors: [COLORREF; 16],
    initialized: bool,
    last_monitor: HMONITOR,
    last_rect: RECT,
    settings: Settings,
}

impl App {
    pub fn new() -> Result<Self, String> {
        let module: HMODULE = unsafe { GetModuleHandleW(None).map_err(|_| "failed to get module handle")? };
        let settings = load_settings().unwrap_or_else(default_settings);
        Ok(Self {
            instance: HINSTANCE(module.0),
            hidden_hwnd: HWND::default(),
            monitor_cache: MonitorCache::new(),
            tray: TrayIcon::default(),
            overlay: OverlayWindow::new(),
            custom_colors: [COLORREF(0); 16],
            initialized: false,
            last_monitor: HMONITOR::default(),
            last_rect: RECT::default(),
            settings,
        })
    }

    pub fn run(&mut self) -> Result<(), String> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        self.overlay.create(self.instance)?;
        self.overlay
            .update_performance(performance_for_mode(self.settings.optimization));

        let class = WNDCLASSW {
            lpfnWndProc: Some(Self::wnd_proc),
            hInstance: self.instance,
            lpszClassName: HIDDEN_CLASS,
            hCursor: unsafe {
                HCURSOR(
                    LoadCursorW(HINSTANCE::default(), IDC_ARROW)
                        .map_err(|_| "failed to load cursor".to_string())?
                        .0,
                )
            },
            ..Default::default()
        };

        unsafe {
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                HIDDEN_CLASS,
                w!("jinagam-rs"),
                WINDOW_STYLE(WS_OVERLAPPED.0),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                0,
                0,
                None,
                HMENU::default(),
                self.instance,
                Some(self as *mut _ as _),
            )
            .map_err(|_| "failed to create hidden window".to_string())?;

            self.hidden_hwnd = hwnd;
            SetTimer(hwnd, POLL_TIMER_ID, POLL_INTERVAL_MS, None);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        Ok(())
    }

    fn on_create(&mut self) -> Result<(), String> {
        self.monitor_cache.refresh();
        self.tray.add(self.hidden_hwnd, "jinagam-rs monitor boundary glow")?;
        self.overlay
            .update_performance(performance_for_mode(self.settings.optimization));
        self.overlay.update_style(self.settings.overlay);
        Ok(())
    }

    fn on_poll(&mut self) {
        if !self.settings.enabled {
            return;
        }

        let mut cursor = POINT::default();
        unsafe {
            if GetCursorPos(&mut cursor).is_err() {
                return;
            }
        }

        let current_monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
        if !self.initialized {
            self.last_monitor = current_monitor;
            self.last_rect = self.monitor_cache.rect_for(current_monitor).unwrap_or_default();
            self.initialized = true;
            return;
        }

        if current_monitor == self.last_monitor {
            return;
        }

        let previous_rect = self.last_rect;
        let current_rect = self.monitor_cache.rect_for(current_monitor).unwrap_or_default();
        if current_rect.right <= current_rect.left || current_rect.bottom <= current_rect.top {
            self.last_monitor = current_monitor;
            self.last_rect = current_rect;
            return;
        }

        if let Some((band, vertical)) = self.compute_boundary_band(previous_rect, current_rect, cursor) {
            self.overlay.show_band(band, vertical);
        }

        self.last_monitor = current_monitor;
        self.last_rect = current_rect;
    }

    fn compute_boundary_band(&self, previous: RECT, current: RECT, cursor: POINT) -> Option<(RECT, bool)> {
        let (vertical, boundary) = pick_boundary_axis(previous, current, cursor);

        let thickness = self.settings.overlay.width.max(12);

        let mut band = if vertical {
            let (top, bottom) = match self.settings.span_mode {
                BoundarySpanMode::CrossingSegment => {
                    let overlap_top = previous.top.max(current.top);
                    let overlap_bottom = previous.bottom.min(current.bottom);
                    if overlap_bottom <= overlap_top {
                        return None;
                    }
                    let movement = (cursor.x - boundary).abs().max(1);
                    let segment_len = (movement * 6).clamp(thickness * 2, 220);
                    let center_y = clamp_center(cursor.y, overlap_top, overlap_bottom, segment_len);
                    (
                        (center_y - segment_len / 2).max(overlap_top),
                        (center_y + segment_len / 2).min(overlap_bottom),
                    )
                }
                BoundarySpanMode::FullBoundary => {
                    (
                        previous.top.min(current.top) - SPAN_PAD,
                        previous.bottom.max(current.bottom) + SPAN_PAD,
                    )
                }
            };
            RECT {
                left: boundary - (thickness / 2),
                right: boundary + ((thickness + 1) / 2),
                top,
                bottom,
            }
        } else {
            let (left, right) = match self.settings.span_mode {
                BoundarySpanMode::CrossingSegment => {
                    let overlap_left = previous.left.max(current.left);
                    let overlap_right = previous.right.min(current.right);
                    if overlap_right <= overlap_left {
                        return None;
                    }
                    let movement = (cursor.y - boundary).abs().max(1);
                    let segment_len = (movement * 6).clamp(thickness * 2, 220);
                    let center_x = clamp_center(cursor.x, overlap_left, overlap_right, segment_len);
                    (
                        (center_x - segment_len / 2).max(overlap_left),
                        (center_x + segment_len / 2).min(overlap_right),
                    )
                }
                BoundarySpanMode::FullBoundary => {
                    (
                        previous.left.min(current.left) - SPAN_PAD,
                        previous.right.max(current.right) + SPAN_PAD,
                    )
                }
            };
            RECT {
                left,
                right,
                top: boundary - (thickness / 2),
                bottom: boundary + ((thickness + 1) / 2),
            }
        };

        let virtual_left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let virtual_top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let virtual_right = virtual_left + unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let virtual_bottom = virtual_top + unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };

        band.left = band.left.max(virtual_left);
        band.top = band.top.max(virtual_top);
        band.right = band.right.min(virtual_right);
        band.bottom = band.bottom.min(virtual_bottom);

        if band.right <= band.left || band.bottom <= band.top {
            None
        } else {
            Some((band, vertical))
        }
    }

    fn handle_command(&mut self, command: usize) {
        match command {
            ID_TOGGLE_ENABLED => {
                self.settings.enabled = !self.settings.enabled;
                if !self.settings.enabled {
                    self.overlay.hide();
                }
            }
            ID_RELOAD_MONITORS => self.monitor_cache.refresh(),
            ID_COLOR_PICKER => {
                if let Some(color) = self.choose_color() {
                    self.settings.overlay.color = color;
                }
            }
            ID_WIDTH_THIN => self.settings.overlay.width = 32,
            ID_WIDTH_NORMAL => self.settings.overlay.width = 48,
            ID_WIDTH_THICK => self.settings.overlay.width = 72,
            ID_DURATION_SHORT => self.settings.overlay.duration_ms = 140,
            ID_DURATION_NORMAL => self.settings.overlay.duration_ms = 220,
            ID_DURATION_LONG => self.settings.overlay.duration_ms = 340,
            ID_SPAN_SEGMENT => self.settings.span_mode = BoundarySpanMode::CrossingSegment,
            ID_SPAN_FULL => self.settings.span_mode = BoundarySpanMode::FullBoundary,
            ID_OPTIMIZE_SMOOTH => self.settings.optimization = OverlayOptimizationMode::Smooth,
            ID_OPTIMIZE_EFFICIENT => self.settings.optimization = OverlayOptimizationMode::Efficient,
            ID_EXIT => unsafe {
                let _ = DestroyWindow(self.hidden_hwnd);
            },
            _ => {}
        }

        self.overlay
            .update_performance(performance_for_mode(self.settings.optimization));
        self.overlay.update_style(self.settings.overlay);
        let _ = save_settings(self.settings);
    }

    fn choose_color(&mut self) -> Option<COLORREF> {
        let mut dialog = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            hwndOwner: self.hidden_hwnd,
            rgbResult: self.settings.overlay.color,
            lpCustColors: self.custom_colors.as_mut_ptr(),
            Flags: CC_RGBINIT | CC_FULLOPEN | CC_ANYCOLOR,
            ..Default::default()
        };

        unsafe {
            if ChooseColorW(&mut dialog).as_bool() {
                Some(dialog.rgbResult)
            } else {
                None
            }
        }
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                let app = create.lpCreateParams as *mut App;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
                (*app).hidden_hwnd = hwnd;
            }

            let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if app.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }

            let app = &mut *app;
            match msg {
                WM_CREATE => match app.on_create() {
                    Ok(()) => LRESULT(0),
                    Err(_) => LRESULT(-1),
                },
                WM_TIMER if wparam.0 == POLL_TIMER_ID => {
                    app.on_poll();
                    LRESULT(0)
                }
                WM_DISPLAYCHANGE => {
                    app.monitor_cache.refresh();
                    LRESULT(0)
                }
                WM_COMMAND => {
                    app.handle_command((wparam.0 & 0xFFFF) as usize);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    app.tray.remove();
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ if app
                    .tray
                    .handle_message(hwnd, msg, wparam, lparam, app.tray_menu_state()) =>
                {
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    fn tray_menu_state(&self) -> TrayMenuState {
        TrayMenuState {
            enabled: self.settings.enabled,
            width: self.settings.overlay.width,
            duration_ms: self.settings.overlay.duration_ms,
            span_full: matches!(self.settings.span_mode, BoundarySpanMode::FullBoundary),
            color: self.settings.overlay.color,
            optimization: match self.settings.optimization {
                OverlayOptimizationMode::Smooth => TrayOptimizationMode::Smooth,
                OverlayOptimizationMode::Efficient => TrayOptimizationMode::Efficient,
            },
        }
    }
}

fn default_settings() -> Settings {
    Settings {
        enabled: true,
        overlay: OverlayStyle {
            color: COLORREF(0x00FFB46E),
            width: 32,
            intensity: 220,
            duration_ms: 220,
        },
        span_mode: BoundarySpanMode::FullBoundary,
        optimization: OverlayOptimizationMode::Efficient,
    }
}

fn load_settings() -> Option<Settings> {
    let path = settings_path()?;
    let content = fs::read_to_string(path).ok()?;
    let mut settings = default_settings();

    for line in content.lines() {
        let (key, value) = line.split_once('=')?;
        match key.trim() {
            "enabled" => settings.enabled = matches!(value.trim(), "1" | "true" | "yes"),
            "color" => {
                let parsed = u32::from_str_radix(value.trim(), 16).ok()?;
                settings.overlay.color = COLORREF(parsed);
            }
            "width" => {
                let parsed = value.trim().parse::<i32>().ok()?;
                settings.overlay.width = parsed.max(12);
            }
            "duration_ms" => {
                let parsed = value.trim().parse::<u32>().ok()?;
                settings.overlay.duration_ms = parsed.max(60);
            }
            "span_mode" => {
                settings.span_mode = if value.trim().eq_ignore_ascii_case("segment") {
                    BoundarySpanMode::CrossingSegment
                } else {
                    BoundarySpanMode::FullBoundary
                };
            }
            "optimization" => {
                settings.optimization = match value.trim().to_ascii_lowercase().as_str() {
                    "smooth" => OverlayOptimizationMode::Smooth,
                    _ => OverlayOptimizationMode::Efficient,
                };
            }
            _ => {}
        }
    }

    Some(settings)
}

fn save_settings(settings: Settings) -> Result<(), String> {
    let path = settings_path().ok_or_else(|| "failed to resolve settings path".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("failed to create settings dir: {err}"))?;
    }

    let span_mode = match settings.span_mode {
        BoundarySpanMode::CrossingSegment => "segment",
        BoundarySpanMode::FullBoundary => "full",
    };
    let optimization = match settings.optimization {
        OverlayOptimizationMode::Smooth => "smooth",
        OverlayOptimizationMode::Efficient => "efficient",
    };
    let body = format!(
        "enabled={}\ncolor={:08X}\nwidth={}\nduration_ms={}\nspan_mode={}\noptimization={}\n",
        if settings.enabled { 1 } else { 0 },
        settings.overlay.color.0,
        settings.overlay.width,
        settings.overlay.duration_ms,
        span_mode,
        optimization,
    );

    fs::write(path, body).map_err(|err| format!("failed to write settings: {err}"))
}

fn settings_path() -> Option<PathBuf> {
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())?;
    Some(base.join("jinagam-rs").join("settings.txt"))
}

fn clamp_center(center: i32, start: i32, end: i32, span: i32) -> i32 {
    let half = span / 2;
    let min_center = start + half.min((end - start) / 2);
    let max_center = end - half.min((end - start) / 2);
    center.clamp(min_center, max_center)
}

fn pick_boundary_axis(previous: RECT, current: RECT, cursor: POINT) -> (bool, i32) {
    let exited_left = cursor.x < previous.left;
    let exited_right = cursor.x >= previous.right;
    let exited_top = cursor.y < previous.top;
    let exited_bottom = cursor.y >= previous.bottom;

    let dx_left = previous.left - cursor.x;
    let dx_right = cursor.x - (previous.right - 1);
    let dy_top = previous.top - cursor.y;
    let dy_bottom = cursor.y - (previous.bottom - 1);

    let mut best_axis = 0;
    let mut best_mag = -1;

    if exited_left {
        best_axis = 1;
        best_mag = best_mag.max(dx_left);
    }
    if exited_right && dx_right > best_mag {
        best_axis = 1;
        best_mag = dx_right;
    }
    if exited_top && dy_top > best_mag {
        best_axis = 2;
        best_mag = dy_top;
    }
    if exited_bottom && dy_bottom > best_mag {
        best_axis = 2;
    }

    if best_axis == 0 {
        let prev_center_x = (previous.left + previous.right) / 2;
        let prev_center_y = (previous.top + previous.bottom) / 2;
        let cur_center_x = (current.left + current.right) / 2;
        let cur_center_y = (current.top + current.bottom) / 2;
        best_axis = if (cur_center_x - prev_center_x).abs() >= (cur_center_y - prev_center_y).abs() {
            1
        } else {
            2
        };
    }

    if best_axis == 1 {
        let edge_x = if exited_left { previous.left } else { previous.right };
        (true, edge_x)
    } else {
        let edge_y = if exited_top { previous.top } else { previous.bottom };
        (false, edge_y)
    }
}
