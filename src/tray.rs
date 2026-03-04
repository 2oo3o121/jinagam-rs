use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, SetForegroundWindow,
    TrackPopupMenu, HICON, HMENU, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
};

const IDI_TRAY_ICON: PCWSTR = PCWSTR(1 as *const u16);

pub const WM_TRAY: u32 = 0x8000 + 77;

pub const ID_TOGGLE_ENABLED: usize = 1001;
pub const ID_RELOAD_MONITORS: usize = 1002;
pub const ID_RESET_DEFAULTS: usize = 1003;
pub const ID_EXIT: usize = 1004;

pub const ID_COLOR_CURRENT: usize = 2000;
pub const ID_COLOR_PICKER: usize = 2001;

pub const ID_WIDTH_THIN: usize = 2101;
pub const ID_WIDTH_NORMAL: usize = 2102;
pub const ID_WIDTH_THICK: usize = 2103;

pub const ID_DURATION_SHORT: usize = 2301;
pub const ID_DURATION_NORMAL: usize = 2302;
pub const ID_DURATION_LONG: usize = 2303;

pub const ID_SPAN_SEGMENT: usize = 2401;
pub const ID_SPAN_FULL: usize = 2402;

pub const ID_OPTIMIZE_SMOOTH: usize = 2501;
pub const ID_OPTIMIZE_EFFICIENT: usize = 2502;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayOptimizationMode {
    Smooth,
    Efficient,
}

#[derive(Clone, Copy)]
pub struct TrayMenuState {
    pub enabled: bool,
    pub width: i32,
    pub duration_ms: u32,
    pub span_full: bool,
    pub color: COLORREF,
    pub optimization: TrayOptimizationMode,
}

#[derive(Default)]
pub struct TrayIcon {
    data: NOTIFYICONDATAW,
    added: bool,
}

impl TrayIcon {
    pub fn add(&mut self, owner: HWND, tip: &str) -> Result<(), String> {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: owner,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: unsafe {
                let instance = HINSTANCE(
                    GetModuleHandleW(None)
                        .map_err(|_| "failed to get module handle")?
                        .0,
                );
                HICON(
                    LoadIconW(instance, IDI_TRAY_ICON)
                        .map_err(|_| "failed to load tray icon")?
                        .0,
                )
            },
            ..Default::default()
        };
        fill_wstr(&mut data.szTip, tip);

        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                return Err("failed to add tray icon".into());
            }
        }

        self.data = data;
        self.added = true;
        Ok(())
    }

    pub fn remove(&mut self) {
        if !self.added {
            return;
        }
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
        self.added = false;
    }

    pub fn handle_message(
        &self,
        hwnd: HWND,
        msg: u32,
        _: WPARAM,
        lparam: LPARAM,
        state: TrayMenuState,
    ) -> bool {
        if msg != WM_TRAY {
            return false;
        }

        match lparam.0 as u32 {
            0x0205 => {
                self.show_menu(hwnd, state);
                true
            }
            0x0202 => true,
            _ => false,
        }
    }

    fn show_menu(&self, hwnd: HWND, state: TrayMenuState) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let Ok(color_menu) = CreatePopupMenu() else {
                let _ = DestroyMenu(menu);
                return;
            };
            let Ok(width_menu) = CreatePopupMenu() else {
                let _ = DestroyMenu(color_menu);
                let _ = DestroyMenu(menu);
                return;
            };
            let Ok(duration_menu) = CreatePopupMenu() else {
                let _ = DestroyMenu(width_menu);
                let _ = DestroyMenu(color_menu);
                let _ = DestroyMenu(menu);
                return;
            };
            let Ok(span_menu) = CreatePopupMenu() else {
                let _ = DestroyMenu(duration_menu);
                let _ = DestroyMenu(width_menu);
                let _ = DestroyMenu(color_menu);
                let _ = DestroyMenu(menu);
                return;
            };
            let Ok(opt_menu) = CreatePopupMenu() else {
                let _ = DestroyMenu(span_menu);
                let _ = DestroyMenu(duration_menu);
                let _ = DestroyMenu(width_menu);
                let _ = DestroyMenu(color_menu);
                let _ = DestroyMenu(menu);
                return;
            };

            append_checked(menu, ID_TOGGLE_ENABLED, w!("Enabled"), state.enabled);
            let _ = AppendMenuW(menu, MF_STRING, ID_RELOAD_MONITORS, w!("Reload monitors"));
            let _ = AppendMenuW(menu, MF_STRING, ID_RESET_DEFAULTS, w!("Reset to defaults"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            let color_label = format_color_label(state.color);
            let color_wide = encode_wide(&color_label);
            let _ = AppendMenuW(
                color_menu,
                MF_STRING | MF_CHECKED | MF_DISABLED | MF_GRAYED,
                ID_COLOR_CURRENT,
                PCWSTR(color_wide.as_ptr()),
            );
            let _ = AppendMenuW(color_menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(
                color_menu,
                MF_STRING,
                ID_COLOR_PICKER,
                w!("Choose color..."),
            );
            let _ = AppendMenuW(menu, MF_POPUP, color_menu.0 as usize, w!("Color"));

            append_checked(
                width_menu,
                ID_WIDTH_THIN,
                w!("Thin (32px)"),
                state.width == 32,
            );
            append_checked(
                width_menu,
                ID_WIDTH_NORMAL,
                w!("Normal (48px)"),
                state.width == 48,
            );
            append_checked(
                width_menu,
                ID_WIDTH_THICK,
                w!("Thick (72px)"),
                state.width == 72,
            );
            let _ = AppendMenuW(menu, MF_POPUP, width_menu.0 as usize, w!("Width"));

            append_checked(
                duration_menu,
                ID_DURATION_SHORT,
                w!("Short"),
                state.duration_ms == 140,
            );
            append_checked(
                duration_menu,
                ID_DURATION_NORMAL,
                w!("Normal"),
                state.duration_ms == 220,
            );
            append_checked(
                duration_menu,
                ID_DURATION_LONG,
                w!("Long"),
                state.duration_ms == 340,
            );
            let _ = AppendMenuW(menu, MF_POPUP, duration_menu.0 as usize, w!("Duration"));

            append_checked(
                span_menu,
                ID_SPAN_SEGMENT,
                w!("Crossing segment"),
                !state.span_full,
            );
            append_checked(
                span_menu,
                ID_SPAN_FULL,
                w!("Full boundary"),
                state.span_full,
            );
            let _ = AppendMenuW(menu, MF_POPUP, span_menu.0 as usize, w!("Span"));

            append_checked(
                opt_menu,
                ID_OPTIMIZE_SMOOTH,
                w!("Smooth"),
                state.optimization == TrayOptimizationMode::Smooth,
            );
            append_checked(
                opt_menu,
                ID_OPTIMIZE_EFFICIENT,
                w!("Efficient"),
                state.optimization == TrayOptimizationMode::Efficient,
            );
            let _ = AppendMenuW(menu, MF_POPUP, opt_menu.0 as usize, w!("Performance"));

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Quit"));

            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                cursor.x,
                cursor.y,
                0,
                hwnd,
                None,
            );

            let _ = DestroyMenu(opt_menu);
            let _ = DestroyMenu(span_menu);
            let _ = DestroyMenu(duration_menu);
            let _ = DestroyMenu(width_menu);
            let _ = DestroyMenu(color_menu);
            let _ = DestroyMenu(menu);
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        self.remove();
    }
}

unsafe fn append_checked(menu: HMENU, id: usize, text: PCWSTR, checked: bool) {
    let flags = if checked {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(menu, flags, id, text);
}

fn fill_wstr(buf: &mut [u16], s: &str) {
    for slot in buf.iter_mut() {
        *slot = 0;
    }

    for (index, code) in s
        .encode_utf16()
        .take(buf.len().saturating_sub(1))
        .enumerate()
    {
        buf[index] = code;
    }
}

fn encode_wide(s: &str) -> Vec<u16> {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    wide.push(0);
    wide
}

fn format_color_label(color: COLORREF) -> String {
    let r = color.0 & 0xFF;
    let g = (color.0 >> 8) & 0xFF;
    let b = (color.0 >> 16) & 0xFF;
    format!("Current #{r:02X}{g:02X}{b:02X}")
}
