#![cfg_attr(not(test), windows_subsystem = "windows")]
//! Sticky: post-it notes, to-dos and reminders.
//!
//!   sticky                       quick add (starts the helper if needed)
//!   sticky add <text>            add straight away ("remind me to … at 4pm"); empty text opens quick add
//!   sticky open|settings|log     the main window on that page
//!   sticky daemon                the background helper: tray, hotkey, reminder clock
//!   sticky note                  the sticky note window
//!   sticky popup <id> <slot> <t> a reminder pop-up (started by the helper)
//!   sticky done|undo <id>        tick or untick an item (the desk dashboard uses this)
//!   sticky kite [keyword]        add the Kite runner command
//!   sticky export                print all items as JSON (for scripts)
//!   sticky update [--auto]       install the latest GitHub release if it's newer
//!   sticky setup                 the installer window (also what sticky-setup.exe shows)
//!   sticky install [--no-autostart] [--no-kite]   install without the window
//!   sticky uninstall [--quiet]   remove it (Settings > Apps runs this)

mod daemon;
mod icon;
mod kite;
mod parse;
mod settings;
mod setup;
mod store;
mod ui;
mod update;
mod win;

use settings::Settings;

fn main() {
    // Per-monitor DPI so placement maths is in real pixels.
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.to_lowercase()).unwrap_or_default();
    let rest = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
    if cmd.is_empty() && setup::is_setup_exe() {
        ui::setup::run();
        return;
    }
    match cmd.as_str() {
        "setup" => ui::setup::run(),
        "install" => {
            let s = Settings::load();
            let o = setup::Options {
                autostart: !args.iter().any(|a| a == "--no-autostart"),
                kite: (!args.iter().any(|a| a == "--no-kite")).then(|| kite::existing_keyword().unwrap_or(s.kite_keyword)),
            };
            match setup::install(&o) {
                Ok((exe, notes)) => println!("Installed {}\n{}", exe.display(), notes.join("\n")),
                Err(e) => {
                    eprintln!("Install failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "uninstall" => setup::uninstall(args.iter().any(|a| a == "--quiet")),
        "" | "capture" | "new" => {
            daemon::ensure_running();
            daemon::open_ui("capture");
        }
        "ui" => ui::main_window::run(args.get(1).map(String::as_str).unwrap_or("full")),
        "open" | "full" => daemon::open_ui("full"),
        "settings" => daemon::open_ui("settings"),
        "log" => daemon::open_ui("log"),
        "daemon" => daemon::run(),
        "note" => ui::note::run(),
        "popup" => {
            let id = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let slot = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let was = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            ui::popup::run(id, slot, was);
        }
        "add" => add(rest.trim().trim_matches('"').trim()),
        // used by the desk dashboard tile
        "done" | "undo" => {
            if let Some(id) = args.get(1).and_then(|s| s.parse().ok()) {
                store::set_done(id, cmd == "done");
            }
        }
        "kite" => {
            let kw = if rest.trim().is_empty() { Settings::load().kite_keyword } else { rest.trim().to_string() };
            let msg = kite::install(&kw).unwrap_or_else(|e| format!("Kite not updated: {e}"));
            store::log("kite", 0, &msg);
            println!("{msg}");
        }
        "update" => update::run(args.iter().any(|a| a == "--auto")),
        "version" | "--version" => println!("sticky {}", update::VERSION),
        "export" => {
            println!("{}", serde_json::to_string_pretty(&store::load()).unwrap_or_default());
        }
        // anything else is text to add: `sticky buy milk tomorrow`
        _ => add(&args.join(" ")),
    }
}

fn add(text: &str) {
    daemon::ensure_running();
    if text.is_empty() {
        daemon::open_ui("capture");
        return;
    }
    let s = Settings::load();
    let p = ui::parse_with(text, &s);
    if p.text.is_empty() {
        daemon::open_ui("capture");
        return;
    }
    let item = ui::item_from(&p, &s, None);
    let title = item.text.clone();
    let mut detail = vec![];
    if let Some(d) = item.due {
        detail.push(format!("{} {}", if item.remind { "Reminder" } else { "Due" }, store::describe_due(d, item.all_day, s.use_24h)));
    }
    if let Some(r) = item.repeat {
        detail.push(r.label().to_string());
    }
    detail.push(item.category.clone());
    if item.priority != store::Priority::None {
        detail.push(format!("{} priority", item.priority.label()));
    }
    let id = store::update(|d| d.add(item));
    store::log("added", id, &title);
    ui::toast::run(title, detail.join(" · "));
}
