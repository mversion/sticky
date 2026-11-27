//! Small Win32 helpers: single instance, focus, placement, sound, autostart, spawning.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub const DAEMON_MUTEX: &str = "Local\\Sticky.Daemon";
pub const UI_MUTEX: &str = "Local\\Sticky.Ui";
pub const NOTE_MUTEX: &str = "Local\\Sticky.Note";

/// The lock name, kept apart per data folder when `STICKY_HOME` points somewhere else, so a test
/// copy can run beside the real one.
fn lock_name(name: &str) -> String {
    match std::env::var("STICKY_HOME") {
        Ok(h) if !h.is_empty() => {
            use std::hash::{Hash, Hasher};
            let mut x = std::collections::hash_map::DefaultHasher::new();
            h.to_lowercase().hash(&mut x);
            format!("{name}.{:x}", x.finish())
        }
        _ => name.to_string(),
    }
}

/// Hold a named mutex for the life of the process. False if another process holds it.
pub fn claim(name: &str) -> bool {
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 1, wide(&lock_name(name)).as_ptr());
        if h.is_null() {
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            return false;
        }
        // leak on purpose: released when the process ends
        true
    }
}

pub fn exists(name: &str) -> bool {
    unsafe {
        let h = OpenMutexW(0x0010_0000, 0, wide(&lock_name(name)).as_ptr());
        if h.is_null() {
            false
        } else {
            CloseHandle(h);
            true
        }
    }
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

pub fn spawn_self(args: &[&str]) -> Option<std::process::Child> {
    let exe = std::env::current_exe().ok()?;
    std::process::Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .ok()
}

/// Let whichever process we start next take the foreground.
pub fn allow_foreground() {
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
    }
}

pub fn hwnd_from(handle: &impl raw_window_handle::HasWindowHandle) -> Option<HWND> {
    match handle.window_handle().ok()?.as_raw() {
        raw_window_handle::RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

/// Bring a window to the front even when Windows' foreground lock would refuse.
pub fn force_foreground(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        if SetForegroundWindow(hwnd) == 0 || GetForegroundWindow() != hwnd {
            // A synthetic Alt tap counts as input and unlocks SetForegroundWindow.
            let mut inputs: [INPUT; 2] = std::mem::zeroed();
            inputs[0].r#type = INPUT_KEYBOARD;
            inputs[0].Anonymous.ki.wVk = VK_MENU;
            inputs[1].r#type = INPUT_KEYBOARD;
            inputs[1].Anonymous.ki.wVk = VK_MENU;
            inputs[1].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
            SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
            SetForegroundWindow(hwnd);
        }
        BringWindowToTop(hwnd);
    }
}

/// Windows 11 rounded corners and a thin border on our borderless windows.
pub fn style_window(hwnd: HWND, border: Option<u32>) {
    unsafe {
        let pref: i32 = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &pref as *const _ as _, 4);
        if let Some(c) = border {
            DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, &c as *const _ as _, 4);
        }
        // A 1px frame extension gives borderless windows the normal drop shadow.
        let m = windows_sys::Win32::UI::Controls::MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 0, cyBottomHeight: 1 };
        DwmExtendFrameIntoClientArea(hwnd, &m);
    }
}

pub fn set_topmost(hwnd: HWND, on: bool) {
    unsafe {
        SetWindowPos(
            hwnd,
            if on { HWND_TOPMOST } else { HWND_NOTOPMOST },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub scale: f32,
}

fn area_of(mon: HMONITOR) -> Area {
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let r = mi.rcWork;
        Area { left: r.left, top: r.top, right: r.right, bottom: r.bottom, scale: dx as f32 / 96.0 }
    }
}

/// Work area (taskbar excluded) of the screen the mouse is on, in physical pixels.
pub fn cursor_area() -> Area {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        area_of(MonitorFromPoint(p, MONITOR_DEFAULTTOPRIMARY))
    }
}

pub fn primary_area() -> Area {
    unsafe { area_of(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)) }
}

/// A connected screen: `id` is its device name (`\\.\DISPLAY2`), which is what settings store.
pub struct Screen {
    pub id: String,
    pub primary: bool,
    pub width: i32,
    pub height: i32,
    hmon: HMONITOR,
}

impl Screen {
    /// "Display 2 · 2560×1440 (main)".
    pub fn label(&self) -> String {
        let n = self.id.trim_start_matches(r"\\.\").replace("DISPLAY", "Display ");
        format!("{n} · {}×{}{}", self.width, self.height, if self.primary { " (main)" } else { "" })
    }
}

pub fn screens() -> Vec<Screen> {
    unsafe extern "system" fn each(mon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows_sys::core::BOOL {
        unsafe {
            let out = &mut *(data as *mut Vec<Screen>);
            let mut mi: MONITORINFOEXW = std::mem::zeroed();
            mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(mon, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO) != 0 {
                let len = mi.szDevice.iter().position(|&c| c == 0).unwrap_or(mi.szDevice.len());
                let r = mi.monitorInfo.rcMonitor;
                out.push(Screen {
                    id: String::from_utf16_lossy(&mi.szDevice[..len]),
                    primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                    hmon: mon,
                });
            }
            1
        }
    }
    let mut v: Vec<Screen> = Vec::new();
    unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut v as *mut Vec<Screen> as LPARAM);
    }
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

/// Work area for a screen setting: "primary" (main display), "cursor" (where the mouse is) or a
/// device name. A screen that's no longer connected falls back to the main display.
pub fn screen_area(choice: &str) -> Area {
    match choice {
        "cursor" => cursor_area(),
        "primary" | "" => primary_area(),
        id => screens().into_iter().find(|s| s.id == id).map(|s| area_of(s.hmon)).unwrap_or_else(primary_area),
    }
}


pub fn move_window(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// The window property that marks a pop-up in the stack; its value orders the stack (oldest first).
const POPUP_PROP: &str = "Sticky.PopupOrder";

/// Put a pop-up in the shared stack, or take it out (`order` 0), e.g. once you drag it away.
pub fn mark_popup(hwnd: HWND, order: isize) {
    let name = wide(POPUP_PROP);
    unsafe {
        if order == 0 {
            RemovePropW(hwnd, name.as_ptr());
        } else {
            SetPropW(hwnd, name.as_ptr(), order as HANDLE);
        }
    }
}

/// Every pop-up in the stack, from any Sticky process: (order, window, height in pixels),
/// oldest first.
pub fn popup_stack() -> Vec<(isize, HWND, i32)> {
    unsafe extern "system" fn each(h: HWND, data: LPARAM) -> windows_sys::core::BOOL {
        unsafe {
            let out = &mut *(data as *mut Vec<(isize, HWND, i32)>);
            let name = wide(POPUP_PROP);
            let order = GetPropW(h, name.as_ptr()) as isize;
            if order != 0 && IsWindowVisible(h) != 0 {
                let mut r: RECT = std::mem::zeroed();
                if GetWindowRect(h, &mut r) != 0 {
                    out.push((order, h, r.bottom - r.top));
                }
            }
            1
        }
    }
    let mut v: Vec<(isize, HWND, i32)> = Vec::new();
    unsafe {
        EnumWindows(Some(each), &mut v as *mut _ as LPARAM);
    }
    v.sort_by_key(|p| (p.0, p.1 as usize));
    v
}

pub fn is_window(hwnd: HWND) -> bool {
    unsafe { IsWindow(hwnd) != 0 }
}

pub fn window_height(hwnd: HWND) -> Option<i32> {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        (GetWindowRect(hwnd, &mut r) != 0).then_some(r.bottom - r.top)
    }
}

pub fn window_pos(hwnd: HWND) -> Option<(i32, i32)> {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        (GetWindowRect(hwnd, &mut r) != 0).then_some((r.left, r.top))
    }
}

/// Wait up to `wait` for a named mutex and hold it until the returned guard is dropped.
pub struct MutexGuard(HANDLE);
impl Drop for MutexGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

pub fn wait_mutex(name: &str, wait: std::time::Duration) -> Option<MutexGuard> {
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, wide(name).as_ptr());
        if h.is_null() {
            return None;
        }
        let r = WaitForSingleObject(h, wait.as_millis() as u32);
        if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
            Some(MutexGuard(h))
        } else {
            CloseHandle(h);
            None
        }
    }
}

pub fn play_alias_sync(alias: &str) {
    use windows_sys::Win32::Media::Audio::*;
    unsafe {
        PlaySoundW(wide(alias).as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_SYNC);
    }
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

pub fn set_autostart(on: bool) {
    set_run(if on { std::env::current_exe().ok() } else { None }.as_deref());
}

/// Point the HKCU Run entry "Sticky" at `exe` (its helper), or remove it.
pub fn set_run(exe: Option<&std::path::Path>) {
    match exe {
        Some(e) => reg_set_str(RUN_KEY, "Sticky", &format!("\"{}\" daemon", e.display())),
        None => reg_delete_value(RUN_KEY, "Sticky"),
    }
}

// HKCU registry helpers. Failures are ignored: none of these is worth stopping for.

fn reg_open(path: &str, create: bool) -> Option<HKEY> {
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let r = if create {
            RegCreateKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, std::ptr::null(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE, std::ptr::null(), &mut key, std::ptr::null_mut())
        } else {
            RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE, &mut key)
        };
        (r == 0).then_some(key)
    }
}

pub fn reg_set_str(path: &str, name: &str, value: &str) {
    if let Some(key) = reg_open(path, true) {
        let v = wide(value);
        unsafe {
            RegSetValueExW(key, wide(name).as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
            RegCloseKey(key);
        }
    }
}

pub fn reg_set_dword(path: &str, name: &str, value: u32) {
    if let Some(key) = reg_open(path, true) {
        unsafe {
            RegSetValueExW(key, wide(name).as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4);
            RegCloseKey(key);
        }
    }
}

pub fn reg_get_str(path: &str, name: &str) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let r = RegGetValueW(HKEY_CURRENT_USER, wide(path).as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as _, &mut size);
        if r != 0 {
            return None;
        }
        let n = buf.iter().position(|c| *c == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

pub fn reg_delete_value(path: &str, name: &str) {
    if let Some(key) = reg_open(path, false) {
        unsafe {
            RegDeleteValueW(key, wide(name).as_ptr());
            RegCloseKey(key);
        }
    }
}

pub fn reg_delete_key(path: &str) {
    unsafe {
        RegDeleteTreeW(HKEY_CURRENT_USER, wide(path).as_ptr());
    }
}

pub struct Proc {
    pub pid: u32,
    pub path: Option<std::path::PathBuf>,
}

/// Running processes whose exe name (lower case) is one of `names`.
pub fn processes(names: &[&str]) -> Vec<Proc> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase();
            if names.contains(&name.as_str()) {
                let mut path = None;
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
                if !h.is_null() {
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0 {
                        path = Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
                    }
                    CloseHandle(h);
                }
                out.push(Proc { pid: e.th32ProcessID, path });
            }
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    out
}

pub fn kill(pid: u32) {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 0);
            CloseHandle(h);
        }
    }
}

/// Resolve a path for comparing (one side may be in short 8.3 form or differ in case).
pub fn real_path(p: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// End every other Sticky process: those running from `folder`, or all of them when None.
/// Returns how many were stopped.
pub fn stop_sticky(folder: Option<&std::path::Path>) -> usize {
    let me = std::process::id();
    let dir = folder.map(real_path);
    let mut n = 0;
    for p in processes(&["sticky.exe", "sticky.old.exe"]) {
        let here = match (&dir, &p.path) {
            (None, _) => true,
            (Some(d), Some(path)) => path.parent().map(real_path).as_ref() == Some(d),
            _ => false,
        };
        if p.pid != me && here {
            kill(p.pid);
            n += 1;
        }
    }
    if n > 0 {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    n
}

/// True when Windows apps are set to light mode.
pub fn system_light() -> bool {
    unsafe {
        let mut data: u32 = 0;
        let mut size = 4u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
            wide("AppsUseLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as _,
            &mut size,
        );
        r == 0 && data == 1
    }
}

/// Start RustDesk on a connection (its ID). A saved password for it is used as usual.
pub fn rustdesk_connect(id: &str) -> bool {
    let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '@' || *c == '.').collect();
    if id.is_empty() {
        return false;
    }
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let exe = [format!("{pf}\\RustDesk\\rustdesk.exe"), format!("{local}\\Programs\\RustDesk\\rustdesk.exe")]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists());
    match exe {
        Some(exe) => std::process::Command::new(exe).args(["--connect", &id]).spawn().is_ok(),
        // the rustdesk:// link RustDesk registers
        None => std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", &format!("rustdesk://{id}")]).spawn().is_ok(),
    }
}

pub fn open_path(p: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe").arg(p).spawn();
}
