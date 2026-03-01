use std::mem::size_of;
use std::time::Instant;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, KillTimer,
    RegisterClassW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, UpdateLayeredWindow,
    CREATESTRUCTW, GWLP_USERDATA, HWND_TOPMOST, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, ULW_ALPHA,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_ERASEBKGND, WM_NCCREATE, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const OVERLAY_CLASS: PCWSTR = w!("jinagam_rs_overlay");
const FADE_TIMER_ID: usize = 1;
const FADE_TICK_MS: u32 = 16;

#[derive(Clone, Copy)]
pub struct OverlayStyle {
    pub color: COLORREF,
    pub width: i32,
    pub intensity: u8,
    pub duration_ms: u32,
}

pub struct OverlayWindow {
    hwnd: HWND,
    band_rect: RECT,
    vertical: bool,
    visible: bool,
    started_at: Option<Instant>,
    style: OverlayStyle,
    mem_dc: HDC,
    dib: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut u8,
    surface_w: i32,
    surface_h: i32,
}

impl OverlayWindow {
    pub fn new() -> Self {
        Self {
            hwnd: HWND::default(),
            band_rect: RECT::default(),
            vertical: true,
            visible: false,
            started_at: None,
            style: OverlayStyle {
                color: COLORREF(0x002830FF),
                width: 48,
                intensity: 220,
                duration_ms: 220,
            },
            mem_dc: HDC::default(),
            dib: HBITMAP::default(),
            old_bmp: HGDIOBJ::default(),
            bits: std::ptr::null_mut(),
            surface_w: 0,
            surface_h: 0,
        }
    }

    pub fn create(&mut self, instance: HINSTANCE) -> Result<(), String> {
        let class = WNDCLASSW {
            lpfnWndProc: Some(Self::wnd_proc),
            hInstance: instance,
            lpszClassName: OVERLAY_CLASS,
            ..Default::default()
        };

        unsafe {
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(
                    WS_EX_TOPMOST.0
                        | WS_EX_LAYERED.0
                        | WS_EX_TRANSPARENT.0
                        | WS_EX_TOOLWINDOW.0
                        | WS_EX_NOACTIVATE.0,
                ),
                OVERLAY_CLASS,
                w!("jinagam-rs-overlay"),
                WINDOW_STYLE(WS_POPUP.0),
                0,
                0,
                1,
                1,
                None,
                None,
                instance,
                Some(self as *mut _ as _),
            )
            .map_err(|_| "failed to create overlay window".to_string())?;

            self.hwnd = hwnd;
            let _ = ShowWindow(hwnd, SW_HIDE);
        }

        Ok(())
    }

    pub fn show_band(&mut self, band_rect: RECT, vertical: bool) {
        self.band_rect = band_rect;
        self.vertical = vertical;
        self.visible = true;
        self.started_at = Some(Instant::now());

        let width = band_rect.right - band_rect.left;
        let height = band_rect.bottom - band_rect.top;

        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                band_rect.left,
                band_rect.top,
                width,
                height,
                SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
            let _ = KillTimer(self.hwnd, FADE_TIMER_ID);
            SetTimer(self.hwnd, FADE_TIMER_ID, FADE_TICK_MS, None);
        }

        let _ = self.render_and_present();
    }

    pub fn update_style(&mut self, style: OverlayStyle) {
        self.style = style;
        if self.visible {
            let _ = self.render_and_present();
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.started_at = None;
        unsafe {
            let _ = KillTimer(self.hwnd, FADE_TIMER_ID);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn handle_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_TIMER if wparam.0 == FADE_TIMER_ID => {
                if self.opacity() <= 0.0 {
                    self.hide();
                } else {
                    let _ = self.render_and_present();
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            _ => unsafe { DefWindowProcW(self.hwnd, msg, wparam, lparam) },
        }
    }

    fn opacity(&self) -> f32 {
        let Some(started_at) = self.started_at else {
            return 0.0;
        };

        let duration = self.style.duration_ms.max(60) as f32;
        let elapsed = started_at.elapsed().as_millis() as f32;
        if elapsed >= duration {
            return 0.0;
        }

        let t = elapsed / duration;
        if t < 0.18 {
            t / 0.18
        } else {
            1.0 - ((t - 0.18) / 0.82)
        }
    }

    fn ensure_surface(&mut self, width: i32, height: i32) -> Result<(), String> {
        if width <= 0 || height <= 0 {
            return Err("overlay surface has invalid size".into());
        }
        if width == self.surface_w && height == self.surface_h && !self.mem_dc.0.is_null() {
            return Ok(());
        }

        self.release_surface();

        unsafe {
            let screen_dc = GetDC(HWND::default());
            let mem_dc = CreateCompatibleDC(screen_dc);
            ReleaseDC(HWND::default(), screen_dc);
            if mem_dc.0.is_null() {
                return Err("failed to create memory dc".into());
            }

            let bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(mem_dc, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(|_| "failed to create overlay dib".to_string())?;
            if bits.is_null() {
                let _ = DeleteDC(mem_dc);
                return Err("failed to create overlay dib".into());
            }

            let old_bmp = SelectObject(mem_dc, dib);
            self.mem_dc = mem_dc;
            self.dib = dib;
            self.old_bmp = old_bmp;
            self.bits = bits as *mut u8;
            self.surface_w = width;
            self.surface_h = height;
        }

        Ok(())
    }

    fn release_surface(&mut self) {
        unsafe {
            if !self.mem_dc.0.is_null() {
                if !self.old_bmp.0.is_null() {
                    SelectObject(self.mem_dc, self.old_bmp);
                }
                if !self.dib.0.is_null() {
                    let _ = DeleteObject(self.dib);
                }
                let _ = DeleteDC(self.mem_dc);
            }
        }

        self.mem_dc = HDC::default();
        self.dib = HBITMAP::default();
        self.old_bmp = HGDIOBJ::default();
        self.bits = std::ptr::null_mut();
        self.surface_w = 0;
        self.surface_h = 0;
    }

    fn render_and_present(&mut self) -> Result<(), String> {
        if !self.visible {
            return Ok(());
        }

        let mut client = RECT::default();
        unsafe { GetClientRect(self.hwnd, &mut client).map_err(|_| "failed to read overlay client rect".to_string())?; }
        let width = client.right - client.left;
        let height = client.bottom - client.top;

        self.ensure_surface(width, height)?;
        self.paint_pixels(width as usize, height as usize);

        let dst = POINT {
            x: self.band_rect.left,
            y: self.band_rect.top,
        };
        let src = POINT::default();
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                HDC::default(),
                Some(&dst as *const POINT),
                Some(&size as *const SIZE),
                self.mem_dc,
                Some(&src as *const POINT),
                COLORREF(0),
                Some(&blend as *const BLENDFUNCTION),
                ULW_ALPHA,
            )
            .map_err(|_| "failed to present overlay".to_string())?;
        }

        Ok(())
    }

    fn paint_pixels(&mut self, width: usize, height: usize) {
        let opacity = self.opacity().clamp(0.0, 1.0);
        let base_alpha = (self.style.intensity as f32 * opacity).round() as u8;
        let feather = (self.style.width.max(12) as f32) * 0.5;
        let core = feather * 0.22;

        let r = (self.style.color.0 & 0xFF) as f32;
        let g = ((self.style.color.0 >> 8) & 0xFF) as f32;
        let b = ((self.style.color.0 >> 16) & 0xFF) as f32;

        let data = unsafe { std::slice::from_raw_parts_mut(self.bits, width * height * 4) };
        data.fill(0);

        let center_x = (width as f32 - 1.0) * 0.5;
        let center_y = (height as f32 - 1.0) * 0.5;

        for y in 0..height {
            for x in 0..width {
                let axis_distance = if self.vertical {
                    (x as f32 - center_x).abs()
                } else {
                    (y as f32 - center_y).abs()
                };

                if axis_distance > feather {
                    continue;
                }

                let edge_opacity = if axis_distance <= core {
                    1.0
                } else {
                    1.0 - ((axis_distance - core) / (feather - core))
                };

                let along_opacity = if self.vertical {
                    let margin = y.min(height - 1 - y) as f32;
                    (0.82 + (margin / (height.max(2) as f32 * 0.5)).min(1.0) * 0.18).min(1.0)
                } else {
                    let margin = x.min(width - 1 - x) as f32;
                    (0.82 + (margin / (width.max(2) as f32 * 0.5)).min(1.0) * 0.18).min(1.0)
                };

                let alpha = (base_alpha as f32 * edge_opacity * along_opacity).round() as u8;
                if alpha == 0 {
                    continue;
                }

                let premul = alpha as f32 / 255.0;
                let idx = (y * width + x) * 4;
                data[idx] = (b * premul).round() as u8;
                data[idx + 1] = (g * premul).round() as u8;
                data[idx + 2] = (r * premul).round() as u8;
                data[idx + 3] = alpha;
            }
        }
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                let this = create.lpCreateParams as *mut OverlayWindow;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, this as isize);
                (*this).hwnd = hwnd;
            }

            let this = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
            if !this.is_null() {
                return (*this).handle_message(msg, wparam, lparam);
            }

            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.hide();
        self.release_surface();
        unsafe {
            if !self.hwnd.0.is_null() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}
