//! Installing and removing Sticky for the current user, no admin needed.
//!
//! The installer is sticky.exe itself: named `sticky-setup.exe` (as on the releases page) or run
//! as `sticky setup`, it shows the setup window. Installing copies the exe to
//! %LOCALAPPDATA%\Programs\Sticky, closes any other running copy, adds the Start-menu shortcut,
//! the Run entry and an entry under Settings > Apps (which runs `sticky uninstall`), points Kite at
//! it and starts the helper. Notes and settings in %USERPROFILE%\.sticky are never touched.

use crate::settings::Settings;
use crate::update::VERSION;
use crate::{kite, store, win};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Sticky";
const NO_WINDOW: u32 = 0x0800_0000;
const DETACHED: u32 = 0x0000_0008;

pub fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Programs").join("Sticky")
}

pub fn installed_exe() -> PathBuf {
    install_dir().join("sticky.exe")
}

fn shortcut_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Microsoft\\Windows\\Start Menu\\Programs\\Sticky.lnk"))
}

/// The version registered under Settings > Apps, if Sticky is installed.
pub fn installed_version() -> Option<String> {
    win::reg_get_str(UNINSTALL_KEY, "DisplayVersion").filter(|_| installed_exe().exists())
}

/// This exe was downloaded as the installer: `sticky-setup.exe`, `sticky-setup (1).exe` and so on.
pub fn is_setup_exe() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
        .is_some_and(|s| s.contains("setup"))
}

/// Sticky copies running from somewhere other than the install folder (an older manual copy).
pub fn other_copies() -> Vec<PathBuf> {
    let dir = win::real_path(&install_dir());
    let me = std::process::id();
    let mut v: Vec<PathBuf> = win::processes(&["sticky.exe"])
        .into_iter()
        .filter(|p| p.pid != me)
        .filter_map(|p| p.path)
        .filter(|p| p.parent().map(win::real_path).as_ref() != Some(&dir))
        .collect();
    v.sort();
    v.dedup();
    v
}

pub struct Options {
    pub autostart: bool,
    /// Add or update the Kite command; the keyword is used only when there isn't one yet.
    pub kite: Option<String>,
}

/// Install this exe. Returns the installed exe's path and notes on what else happened.
pub fn install(o: &Options) -> Result<(PathBuf, Vec<String>), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = install_dir();
    let target = dir.join("sticky.exe");
    let mut notes = vec![];
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;

    // close every other copy (an older manual one too), so only the installed one runs
    win::stop_sticky(None);
    if win::real_path(&me) != win::real_path(&target) {
        let mut last = String::new();
        let ok = (0..6).any(|i| {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
            std::fs::copy(&me, &target).map_err(|e| last = e.to_string()).is_ok()
        });
        if !ok {
            return Err(format!("copy to {}: {last}", target.display()));
        }
    }
    let _ = std::fs::remove_file(dir.join("sticky.old.exe"));

    register(&target);
    if let Err(e) = shortcut(&target) {
        notes.push(format!("Start-menu shortcut not made: {e}"));
    }
    let mut s = Settings::load();
    if s.start_with_windows != o.autostart {
        s.start_with_windows = o.autostart;
        s.save();
    }
    win::set_run(o.autostart.then_some(target.as_path()));
    if let Some(kw) = &o.kite
        && kite::found()
    {
        match kite::with_kite_closed(|| kite::install_at(&target, kw)) {
            Ok(m) => notes.push(m),
            Err(e) => notes.push(format!("Kite not updated: {e}")),
        }
    }
    let _ = Command::new(&target).arg("daemon").current_dir(&dir).creation_flags(NO_WINDOW | DETACHED).spawn();
    store::log("installed", 0, &format!("Sticky {VERSION} in {}", dir.display()));
    Ok((target, notes))
}

fn register(exe: &Path) {
    let k = UNINSTALL_KEY;
    let q = format!("\"{}\"", exe.display());
    win::reg_set_str(k, "DisplayName", "Sticky");
    win::reg_set_str(k, "DisplayVersion", VERSION);
    win::reg_set_str(k, "Publisher", "mversion");
    win::reg_set_str(k, "DisplayIcon", &format!("{q},0"));
    win::reg_set_str(k, "InstallLocation", &exe.parent().unwrap_or(Path::new(".")).display().to_string());
    win::reg_set_str(k, "UninstallString", &format!("{q} uninstall"));
    win::reg_set_str(k, "QuietUninstallString", &format!("{q} uninstall --quiet"));
    win::reg_set_str(k, "URLInfoAbout", "https://github.com/mversion/sticky");
    win::reg_set_str(k, "InstallDate", &chrono::Local::now().format("%Y%m%d").to_string());
    win::reg_set_dword(k, "NoModify", 1);
    win::reg_set_dword(k, "NoRepair", 1);
    let kb = std::fs::metadata(exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    win::reg_set_dword(k, "EstimatedSize", kb);
}

/// Keep the version under Settings > Apps right after the updater swaps in a new exe.
pub fn refresh_registration() {
    let Ok(me) = std::env::current_exe() else { return };
    let Some(loc) = win::reg_get_str(UNINSTALL_KEY, "InstallLocation") else { return };
    if me.parent().map(win::real_path) == Some(win::real_path(Path::new(&loc))) && win::reg_get_str(UNINSTALL_KEY, "DisplayVersion").as_deref() != Some(VERSION) {
        win::reg_set_str(UNINSTALL_KEY, "DisplayVersion", VERSION);
    }
}

fn shortcut(exe: &Path) -> Result<(), String> {
    let lnk = shortcut_path().ok_or("APPDATA not set")?;
    let q = |p: &Path| p.display().to_string().replace('\'', "''");
    let dir = exe.parent().unwrap_or(Path::new("."));
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.Arguments='open';$s.WorkingDirectory='{}';$s.IconLocation='{},0';$s.Description='Sticky notes, to-dos and reminders';$s.Save()",
        q(&lnk),
        q(exe),
        q(dir),
        q(exe)
    );
    let st = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err(format!("PowerShell exited with {st}")) }
}

fn message(text: &str, question: bool) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let flags = if question { MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 } else { MB_OK | MB_ICONINFORMATION };
    unsafe { MessageBoxW(std::ptr::null_mut(), win::wide(text).as_ptr(), win::wide("Sticky").as_ptr(), flags | MB_SETFOREGROUND) == IDYES }
}

/// `sticky uninstall [--quiet]`, run from Settings > Apps.
pub fn uninstall(quiet: bool) {
    let notes = store::data_dir();
    if !quiet && !message(&format!("Remove Sticky from this PC?\n\nYour notes and settings stay in {}.", notes.display()), true) {
        return;
    }
    let dir = install_dir();
    let exe = dir.join("sticky.exe");
    win::stop_sticky(None);
    if win::reg_get_str("Software\\Microsoft\\Windows\\CurrentVersion\\Run", "Sticky").is_some_and(|v| v.to_lowercase().contains(&dir.display().to_string().to_lowercase())) {
        win::set_run(None);
    }
    if let Some(l) = shortcut_path() {
        let _ = std::fs::remove_file(l);
    }
    if kite::found() {
        let _ = kite::with_kite_closed(|| kite::remove(&exe));
    }
    win::reg_delete_key(UNINSTALL_KEY);
    store::log("uninstalled", 0, &dir.display().to_string());
    // this exe may be the one being removed: delete the folder once we've exited
    let me = std::env::current_exe().ok().map(|p| win::real_path(&p));
    if me.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)) == Some(win::real_path(&dir)) {
        let cmd = format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
        let _ = Command::new("cmd.exe").raw_arg(format!("/c {cmd}")).creation_flags(NO_WINDOW | DETACHED).spawn();
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
    if !quiet {
        message(&format!("Sticky has been removed.\n\nYour notes are still in {}. Delete that folder too if you don't want them.", notes.display()), false);
    }
}
