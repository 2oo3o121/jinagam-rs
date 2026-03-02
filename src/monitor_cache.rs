use std::sync::Mutex;

use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};

#[derive(Clone, Copy, Default)]
pub struct MonitorRect {
    pub handle: HMONITOR,
    pub rect: RECT,
}

#[derive(Default)]
pub struct MonitorCache {
    monitors: Mutex<Vec<MonitorRect>>,
}

impl MonitorCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn refresh(&self) {
        let mut scratch = Vec::<MonitorRect>::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                HDC::default(),
                None,
                Some(enum_monitor_proc),
                LPARAM((&mut scratch as *mut Vec<MonitorRect>) as isize),
            );
        }
        *self.monitors.lock().expect("monitor cache poisoned") = scratch;
    }

    pub fn rect_for(&self, handle: HMONITOR) -> Option<RECT> {
        self.monitors
            .lock()
            .expect("monitor cache poisoned")
            .iter()
            .find(|monitor| monitor.handle == handle)
            .map(|monitor| monitor.rect)
    }
}

unsafe extern "system" fn enum_monitor_proc(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    state: LPARAM,
) -> BOOL {
    let monitors = &mut *(state.0 as *mut Vec<MonitorRect>);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };

    if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFO as *mut _).as_bool() {
        monitors.push(MonitorRect {
            handle: monitor,
            rect: info.rcMonitor,
        });
    }

    true.into()
}
