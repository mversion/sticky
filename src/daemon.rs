//! The background helper: tray icon, global hotkey and the reminder clock.
//! No window and no GPU, so it sits at a few MB. Windows open as their own short-lived processes.

use crate::settings::Settings;
use crate::store::{self, Data};
use crate::win;
use chrono::Local;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use std::process::Child;
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Open the main window in a mode, or switch the open one to it.
pub fn open_ui(mode: &str) {
    win::allow_foreground();
    if win::exists(win::UI_MUTEX) {
        let _ = std::fs::write(store::request_path(), mode);
    } else {
        win::spawn_self(&["ui", mode]);
    }
}

pub fn ensure_running() {
    if !win::exists(win::DAEMON_MUTEX) {
        win::spawn_self(&["daemon"]);
    }
}

struct State {
    settings: Settings,
    settings_mtime: Option<SystemTime>,
    items_mtime: Option<SystemTime>,
    data: Data,
    manager: GlobalHotKeyManager,
    hotkey: Option<HotKey>,
    tray: TrayIcon,
    note_item: CheckMenuItem,
    ids: Ids,
    popups: Vec<Child>,
    note: Option<Child>,
    last_tip: String,
    update_item: MenuItem,
    next_check: std::time::Instant,
    offered: Option<String>,
    auto_tried: Option<String>,
}

struct Ids {
    add: tray_icon::menu::MenuId,
    open: tray_icon::menu::MenuId,
    note: tray_icon::menu::MenuId,
    settings: tray_icon::menu::MenuId,
    log: tray_icon::menu::MenuId,
    update: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
}

pub fn run() {
    if !win::claim(win::DAEMON_MUTEX) {
        return;
    }
    Settings::ensure_file();
    crate::update::cleanup();
    crate::setup::refresh_registration();
    let settings = Settings::load();
    win::set_autostart(settings.start_with_windows);

    let menu = Menu::new();
    let add = MenuItem::new(format!("Quick add\t{}", settings.hotkey), true, None);
    let open = MenuItem::new("Open Sticky", true, None);
    let note = CheckMenuItem::new("Sticky note on desktop", true, settings.note_window, None);
    let set = MenuItem::new("Settings", true, None);
    let log = MenuItem::new("Activity log", true, None);
    let update = MenuItem::new(format!("Check for updates (v{})", crate::update::VERSION), true, None);
    let quit = MenuItem::new("Quit Sticky", true, None);
    let _ = menu.append_items(&[&add, &open, &note, &PredefinedMenuItem::separator(), &set, &log, &update, &PredefinedMenuItem::separator(), &quit]);
    let ids = Ids {
        add: add.id().clone(),
        open: open.id().clone(),
        note: note.id().clone(),
        settings: set.id().clone(),
        log: log.id().clone(),
        update: update.id().clone(),
        quit: quit.id().clone(),
    };
    let icon = Icon::from_rgba(crate::icon::rgba(32, false), 32, 32).ok();
    let mut tb = TrayIconBuilder::new().with_menu(Box::new(menu)).with_menu_on_left_click(false).with_tooltip("Sticky");
    if let Some(i) = icon {
        tb = tb.with_icon(i);
    }
    let tray = match tb.build() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("tray: {e}");
            return;
        }
    };
    let manager = match GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("hotkey manager: {e}");
            return;
        }
    };

    let mut st = State {
        settings_mtime: store::mtime(&store::settings_path()),
        items_mtime: None,
        data: Data::default(),
        settings,
        manager,
        hotkey: None,
        tray,
        note_item: note,
        ids,
        popups: vec![],
        note: None,
        last_tip: String::new(),
        update_item: update,
        next_check: std::time::Instant::now() + std::time::Duration::from_secs(90),
        offered: None,
        auto_tried: None,
    };
    st.register_hotkey();
    if st.settings.note_window && !win::exists(win::NOTE_MUTEX) {
        st.note = win::spawn_self(&["note"]);
    }
    store::log("helper started", 0, "");

    // Events arrive on this thread inside DispatchMessage; queue them and wake the loop at once,
    // so the hotkey doesn't wait for the next timer tick.
    TID.store(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }, Ordering::SeqCst);
    GlobalHotKeyEvent::set_event_handler(Some(|e| push(Ev::Hot(e))));
    TrayIconEvent::set_event_handler(Some(|e| push(Ev::Tray(e))));
    MenuEvent::set_event_handler(Some(|e| push(Ev::Menu(e))));

    unsafe {
        SetTimer(std::ptr::null_mut(), 1, 500, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if (msg.message == WM_TIMER || msg.message == WM_APP) && !st.tick() {
                break;
            }
        }
    }
}

enum Ev {
    Hot(GlobalHotKeyEvent),
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}
static QUEUE: Mutex<Vec<Ev>> = Mutex::new(Vec::new());
static TID: AtomicU32 = AtomicU32::new(0);

fn push(e: Ev) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(e);
    }
    unsafe {
        PostThreadMessageW(TID.load(Ordering::SeqCst), WM_APP, 0, 0);
    }
}

impl State {
    fn register_hotkey(&mut self) {
        if let Some(h) = self.hotkey.take() {
            let _ = self.manager.unregister(h);
        }
        match HotKey::from_str(&self.settings.hotkey.replace(' ', "")) {
            Ok(h) => match self.manager.register(h) {
                Ok(()) => self.hotkey = Some(h),
                Err(e) => store::log("hotkey failed", 0, &format!("{}: {e}", self.settings.hotkey)),
            },
            Err(e) => store::log("hotkey invalid", 0, &format!("{}: {e}", self.settings.hotkey)),
        }
    }

    /// Runs every half second (and on the hotkey). False to quit.
    fn tick(&mut self) -> bool {
        let events: Vec<Ev> = QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for ev in events {
            let e = match ev {
                Ev::Hot(e) => {
                    if e.state == HotKeyState::Pressed && Some(e.id) == self.hotkey.map(|h| h.id()) {
                        open_ui("capture");
                    }
                    continue;
                }
                Ev::Tray(e) => {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        open_ui("full");
                    }
                    continue;
                }
                Ev::Menu(e) => e,
            };
            let id = e.id;
            if id == self.ids.add {
                open_ui("capture");
            } else if id == self.ids.open {
                open_ui("full");
            } else if id == self.ids.settings {
                open_ui("settings");
            } else if id == self.ids.log {
                open_ui("log");
            } else if id == self.ids.note {
                let mut s = Settings::load();
                s.note_window = self.note_item.is_checked();
                s.save();
            } else if id == self.ids.update {
                win::allow_foreground();
                win::spawn_self(&["update"]);
            } else if id == self.ids.quit {
                store::log("helper stopped", 0, "");
                return false;
            }
        }

        // settings changed (from the UI or by hand)
        let m = store::mtime(&store::settings_path());
        if m != self.settings_mtime {
            self.settings_mtime = m;
            let new = Settings::load();
            if new.hotkey != self.settings.hotkey {
                self.settings.hotkey = new.hotkey.clone();
                self.register_hotkey();
            }
            if new.start_with_windows != self.settings.start_with_windows {
                win::set_autostart(new.start_with_windows);
            }
            self.note_item.set_checked(new.note_window);
            self.settings = new;
        }
        // the sticky note window follows the setting
        if let Some(c) = &mut self.note
            && !matches!(c.try_wait(), Ok(None))
        {
            self.note = None;
        }
        if self.settings.note_window && self.note.is_none() && !win::exists(win::NOTE_MUTEX) {
            self.note = win::spawn_self(&["note"]);
        }

        // reminders
        let m = store::mtime(&store::items_path());
        if m != self.items_mtime {
            self.items_mtime = m;
            self.data = store::load();
        }
        let now = Local::now();
        let due: Vec<u64> = self.data.items.iter().filter(|i| i.alarm_at().is_some_and(|t| t <= now)).map(|i| i.id).collect();
        if !due.is_empty() {
            self.popups.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
            for id in due {
                let fired = store::update(|d| {
                    let i = d.get_mut(id)?;
                    let was = i.snoozed_until.or(i.due)?;
                    let from_snooze = i.snoozed_until.take().is_some();
                    i.fired = true;
                    if i.repeat.is_some() && from_snooze {
                        // the due date already moved on when it first went off
                        i.fired = false;
                    } else if let (Some(r), Some(due)) = (i.repeat, i.due) {
                        // line up the next occurrence straight away
                        let mut next = r.next(due);
                        while next <= now {
                            next = r.next(next);
                        }
                        i.due = Some(next);
                        i.fired = false;
                    }
                    Some((i.text.clone(), was))
                });
                if let Some((text, was)) = fired {
                    store::log("reminded", id, &text);
                    let slot = self.popups.len().to_string();
                    let was = was.timestamp().to_string();
                    win::allow_foreground();
                    if let Some(c) = win::spawn_self(&["popup", &id.to_string(), &slot, &was]) {
                        self.popups.push(c);
                    }
                }
            }
            self.items_mtime = None; // re-read what we just wrote
        }

        self.updates();

        // tooltip: open count and the next reminder
        let open = self.data.items.iter().filter(|i| !i.done && i.kind == store::Kind::Task).count();
        let next = self.data.items.iter().filter_map(|i| i.alarm_at()).filter(|t| *t > now).min();
        let mut tip = format!("Sticky: {open} open");
        if let Some(n) = next {
            tip += &format!(", next reminder {}", store::describe_due(n, false, self.settings.use_24h));
        }
        if tip != self.last_tip {
            let _ = self.tray.set_tooltip(Some(&tip));
            self.last_tip = tip;
        }
        true
    }
    /// Twice a day, ask GitHub for the latest release (off this thread). A newer one shows in
    /// the tray menu and, with auto update on, installs itself once nothing is open.
    fn updates(&mut self) {
        if let Some(v) = LATEST.lock().ok().and_then(|mut l| l.take())
            && crate::update::is_newer(&v, crate::update::VERSION)
            && self.offered.as_deref() != Some(&v)
        {
            self.update_item.set_text(format!("Update to v{v} (now v{})", crate::update::VERSION));
            store::log("update available", 0, &format!("v{v}"));
            self.offered = Some(v);
        }
        if let Some(v) = self.offered.clone()
            && self.settings.auto_update
            && self.auto_tried.as_deref() != Some(&v)
            && self.popups.iter_mut().all(|c| !matches!(c.try_wait(), Ok(None)))
            && !win::exists(win::UI_MUTEX)
        {
            self.auto_tried = Some(v);
            win::spawn_self(&["update", "--auto"]);
        }
        if self.settings.check_updates && std::time::Instant::now() >= self.next_check {
            self.next_check = std::time::Instant::now() + std::time::Duration::from_secs(12 * 3600);
            std::thread::spawn(|| {
                if let Ok(r) = crate::update::latest()
                    && let Ok(mut l) = LATEST.lock()
                {
                    *l = Some(r.version);
                }
            });
        }
    }
}

static LATEST: Mutex<Option<String>> = Mutex::new(None);