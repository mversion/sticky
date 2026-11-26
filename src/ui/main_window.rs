//! The main window. Opens small as the quick-add box (hotkey, Kite) and grows into the full
//! app: lists by category, reminders, notes, done, the activity log and settings.

use super::*;
use crate::settings::Settings;
use crate::store::{self, Data, Item, Kind, LogEntry, Priority, Repeat};
use crate::win;
use eframe::egui::{self, Align, Color32, CornerRadius, Key, Layout, Margin, RichText, Stroke, Vec2, ViewportCommand};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use windows_sys::Win32::Foundation::HWND;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Mode {
    Capture,
    Full,
}

#[derive(PartialEq, Clone, Debug)]
pub enum View {
    Open,
    Category(String),
    Reminders,
    Notes,
    Done,
    Log,
    Settings,
}

struct Editor {
    id: u64,
    text: String,
    notes: String,
    steps: Vec<store::Step>,
    new_step: String,
    when: String,
    remind: bool,
    category: String,
    priority: Priority,
    repeat: Option<Repeat>,
    kind: Kind,
}

pub struct MainApp {
    mode: Mode,
    view: View,
    input: String,
    search: String,
    pub(super) settings: Settings,
    pub(super) pal: Pal,
    data: Data,
    log: Vec<LogEntry>,
    watch: Arc<Watch>,
    hwnd: Option<HWND>,
    editor: Option<Editor>,
    focus_input: bool,
    was_focused: bool,
    last_height: f32,
    flash: Option<(String, std::time::Instant)>,
    confirm: Option<u64>,
    pub(super) set_search: String,
    pub(super) new_cat: String,
    pub(super) ha: super::settings_view::HaUi,
    frames: u32,
    /// Sidebar: the name being typed for a new folder, and a folder being renamed (old, new).
    new_folder: Option<String>,
    renaming: Option<(String, String)>,
    ctx: egui::Context,
}

const CAPTURE_W: f32 = 620.0;

pub fn run(mode: &str) {
    if !win::claim(win::UI_MUTEX) {
        crate::daemon::open_ui(mode);
        return;
    }
    crate::daemon::ensure_running();
    let settings = Settings::load();
    let m = if mode == "capture" { Mode::Capture } else { Mode::Full };
    let size = match m {
        Mode::Capture => Vec2::new(CAPTURE_W, 120.0),
        Mode::Full => Vec2::from(settings.main_size),
    };
    let vb = egui::ViewportBuilder::default()
        .with_title("Sticky")
        .with_app_id("Sticky")
        .with_inner_size(size)
        .with_min_inner_size(Vec2::new(420.0, 90.0))
        .with_decorations(false)
        .with_resizable(true)
        .with_taskbar(m == Mode::Full)
        // STICKY_NO_FOCUS: open without taking keyboard focus (test screenshots)
        .with_active(std::env::var_os("STICKY_NO_FOCUS").is_none());
    let mode = mode.to_string();
    let _ = eframe::run_native(
        "Sticky",
        native_options(vb),
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let pal = palette(&settings);
            apply_style(&cc.egui_ctx, &pal, &settings);
            let hwnd = win::hwnd_from(cc);
            if let Some(h) = hwnd {
                win::style_window(h, None);
                place(h, m, size, &settings.window_screen);
            }
            let watch = watch(cc.egui_ctx.clone(), true);
            let view = match mode.as_str() {
                "settings" => View::Settings,
                "log" => View::Log,
                _ => View::Open,
            };
            let mut app = MainApp {
                mode: m,
                view,
                input: String::new(),
                search: String::new(),
                settings,
                pal,
                data: store::load(),
                log: vec![],
                watch,
                hwnd,
                editor: None,
                focus_input: true,
                was_focused: false,
                last_height: 0.0,
                flash: None,
                confirm: None,
                set_search: std::env::var("STICKY_SETTINGS_SEARCH").unwrap_or_default(), // for screenshots
                new_cat: String::new(),
                ha: Default::default(),
                frames: 0,
                new_folder: None,
                renaming: None,
                ctx: cc.egui_ctx.clone(),
            };
            // for screenshots: open an item's editor straight away
            if let Some(id) = std::env::var("STICKY_EDIT").ok().and_then(|v| v.parse().ok()) {
                app.act(id, RowAct::Open);
            }
            Ok(Box::new(app))
        }),
    );
}

/// Centre on the chosen screen (Settings > Open Sticky on); quick add sits in the top third like a launcher.
fn place(h: HWND, m: Mode, size: Vec2, screen: &str) {
    let a = win::screen_area(screen);
    let w = (size.x * a.scale) as i32;
    let hh = (size.y * a.scale) as i32;
    let x = a.left + ((a.right - a.left) - w) / 2;
    let y = match m {
        Mode::Capture => a.top + ((a.bottom - a.top) as f32 * 0.24) as i32,
        Mode::Full => a.top + ((a.bottom - a.top) - hh) / 2,
    };
    win::move_window(h, x, y.max(a.top));
}

impl MainApp {
    fn reload(&mut self) {
        self.data = store::load();
        if self.view == View::Log {
            self.log = store::read_log(2000);
        }
    }

    fn switch(&mut self, mode: Mode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if let Some(h) = self.hwnd {
            let size = match mode {
                Mode::Capture => Vec2::new(CAPTURE_W, self.last_height.max(120.0)),
                Mode::Full => Vec2::from(self.settings.main_size),
            };
            self.last_height = 0.0;
            self.ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            place(h, mode, size, &self.settings.window_screen);
        }
    }

    fn add_from_input(&mut self, ctx: &egui::Context) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let p = parse_with(&text, &self.settings);
        if p.text.is_empty() {
            return;
        }
        let fallback = match &self.view {
            View::Category(c) => Some(c.clone()),
            _ => None,
        };
        let mut item = item_from(&p, &self.settings, fallback.as_deref());
        // in Notes, what you add is a note (a colon in a link or a time used to make it a task)
        if self.view == View::Notes {
            item.kind = Kind::Note;
            item.remind = false;
        }
        let summary = item.text.clone();
        let id = store::update(|d| d.add(item));
        store::log("added", id, &summary);
        self.input.clear();
        self.reload();
        if self.mode == Mode::Capture && self.settings.close_capture_after_add {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        } else {
            self.flash = Some((format!("Added: {summary}"), std::time::Instant::now()));
            self.focus_input = true;
        }
    }

    fn act(&mut self, id: u64, a: RowAct) {
        match a {
            RowAct::Toggle => {
                let done = self.data.get(id).map(|i| !i.done).unwrap_or(true);
                store::set_done(id, done);
            }
            RowAct::Delete => {
                if self.settings.confirm_delete && self.confirm != Some(id) {
                    self.confirm = Some(id);
                    return;
                }
                store::delete(id);
                self.confirm = None;
                if self.editor.as_ref().is_some_and(|e| e.id == id) {
                    self.editor = None;
                }
            }
            RowAct::Pin => {
                store::update(|d| {
                    if let Some(i) = d.get_mut(id) {
                        i.pinned = !i.pinned;
                    }
                });
            }
            RowAct::Open => {
                if self.editor.as_ref().is_some_and(|e| e.id == id) {
                    self.editor = None;
                } else if let Some(i) = self.data.get(id) {
                    self.editor = Some(Editor {
                        id,
                        text: i.text.clone(),
                        notes: i.notes.clone(),
                        steps: i.steps.clone(),
                        new_step: String::new(),
                        when: i.due.map(|d| if i.all_day { d.format("%-d/%-m/%Y").to_string() } else { d.format("%-d/%-m/%Y %-I:%M%P").to_string() }).unwrap_or_default(),
                        remind: i.remind,
                        category: i.category.clone(),
                        priority: i.priority,
                        repeat: i.repeat,
                        kind: i.kind,
                    });
                }
            }
            RowAct::None => {}
        }
        self.reload();
    }

    /// Add, rename, move or delete a folder from the sidebar. Folders are `settings.categories`;
    /// an item's folder is its `category` (the desk dashboard reads that name).
    fn folder_action(&mut self, a: FolderAct) {
        let mut s = Settings::load();
        let taken = |s: &Settings, n: &str| s.categories.iter().any(|c| c.eq_ignore_ascii_case(n));
        match a {
            FolderAct::Add(name) => {
                self.new_folder = None;
                if !name.is_empty() && !taken(&s, &name) {
                    s.categories.push(name.clone());
                    s.save();
                    store::log("folder added", 0, &name);
                    self.view = View::Category(name);
                }
            }
            FolderAct::CancelNew => self.new_folder = None,
            FolderAct::StartRename(old) => {
                self.new_folder = None;
                self.renaming = Some((old.clone(), old));
            }
            FolderAct::CancelRename => self.renaming = None,
            FolderAct::Rename(old, new) => {
                self.renaming = None;
                if new.is_empty() || new == old || (taken(&s, &new) && !new.eq_ignore_ascii_case(&old)) {
                    return;
                }
                for c in s.categories.iter_mut() {
                    if *c == old {
                        *c = new.clone();
                    }
                }
                if s.default_category == old {
                    s.default_category = new.clone();
                }
                if let Some(st) = s.folder_styles.remove(&old) {
                    s.folder_styles.insert(new.clone(), st);
                }
                s.save();
                store::update(|d| {
                    for i in d.items.iter_mut().filter(|i| i.category == old) {
                        i.category = new.clone();
                    }
                });
                store::log("folder renamed", 0, &format!("{old} to {new}"));
                if self.view == View::Category(old) {
                    self.view = View::Category(new);
                }
            }
            FolderAct::Colour(name, c) => {
                s.folder_styles.entry(name.clone()).or_default().color = c;
                s.folder_styles.retain(|_, st| !st.color.is_empty() || !st.icon.is_empty());
                s.save();
            }
            FolderAct::Icon(name, g) => {
                s.folder_styles.entry(name.clone()).or_default().icon = g;
                s.folder_styles.retain(|_, st| !st.color.is_empty() || !st.icon.is_empty());
                s.save();
            }
            FolderAct::Move(from, to) => {
                if from < s.categories.len() && to < s.categories.len() {
                    s.categories.swap(from, to);
                    s.save();
                }
            }
            FolderAct::Delete(name) => {
                if s.categories.len() < 2 {
                    return;
                }
                s.categories.retain(|c| *c != name);
                s.folder_styles.remove(&name);
                if s.default_category == name {
                    s.default_category = s.categories[0].clone();
                }
                let to = s.default_category.clone();
                s.save();
                let moved = store::update(|d| {
                    let mut n = 0;
                    for i in d.items.iter_mut().filter(|i| i.category == name) {
                        i.category = to.clone();
                        n += 1;
                    }
                    n
                });
                store::log("folder deleted", 0, &format!("{name} ({moved} item(s) moved to {to})"));
                if self.view == View::Category(name) {
                    self.view = View::Open;
                }
            }
        }
        self.settings = Settings::load();
        self.reload();
    }

    fn save_editor(&mut self) {
        let Some(e) = self.editor.take() else { return };
        let s = &self.settings;
        let when = e.when.trim();
        let parsed = (!when.is_empty()).then(|| parse_with(&format!("x {}", fix_slash_time(when)), s));
        store::update(|d| {
            if let Some(i) = d.get_mut(e.id) {
                let changed_due;
                i.text = e.text.trim().to_string();
                i.notes = e.notes.trim_end().to_string();
                i.category = e.category.clone();
                i.priority = e.priority;
                i.repeat = e.repeat;
                i.kind = e.kind;
                match &parsed {
                    Some(p) if p.due.is_some() => {
                        let nd = p.due.and_then(|x| x.and_local_timezone(chrono::Local).single());
                        changed_due = nd != i.due;
                        i.due = nd;
                        i.all_day = p.all_day && !e.remind;
                    }
                    Some(_) => changed_due = false,
                    None => {
                        changed_due = i.due.is_some();
                        i.due = None;
                    }
                }
                // A repeat with no date starts today, To Do style; minutes/hours start one interval from now.
                if i.due.is_none()
                    && let Some(r) = i.repeat
                {
                    let now = chrono::Local::now();
                    let first = if r.intraday() {
                        r.next_naive(now.naive_local())
                    } else {
                        let start = now.date_naive().and_time(s.default_time());
                        r.first_from(start, if e.remind { now.naive_local() } else { start - chrono::Duration::seconds(1) })
                    };
                    i.due = first.and_local_timezone(chrono::Local).earliest();
                    i.all_day = !e.remind && !r.intraday();
                    i.fired = false;
                }
                let intraday = i.repeat.is_some_and(|r| r.intraday());
                i.remind = (e.remind || intraday) && i.due.is_some() && e.kind == Kind::Task;
                if changed_due || i.remind {
                    if changed_due {
                        i.fired = false;
                    }
                    i.snoozed_until = None;
                    if i.remind && i.all_day {
                        i.all_day = false;
                    }
                }
            }
        });
        let steps: Vec<store::Step> = e.steps.iter().filter(|x| !x.text.trim().is_empty()).cloned().collect();
        let pending = e.new_step.trim().to_string();
        let mut steps = steps;
        if !pending.is_empty() {
            // typed a step and pressed Save without Enter
            steps.push(store::Step { text: pending, done: false });
        }
        if Some(&steps) != self.data.get(e.id).map(|i| &i.steps) {
            store::set_steps(e.id, steps, true);
        }
        store::log("edited", e.id, &e.text);
        self.reload();
    }

    // ------------------------------------------------------------ pieces

    fn input_box(&mut self, ui: &mut egui::Ui, big: bool) {
        let pal = self.pal;
        let h = if big { 50.0 } else { 40.0 };
        egui::Frame::new()
            .fill(pal.card)
            .stroke(Stroke::new(1.0, pal.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 0))
            .show(ui, |ui| {
                ui.set_height(h);
                ui.horizontal_centered(|ui| {
                    ui.label(icon(ic::ADD, if big { 18.0 } else { 15.0 }, pal.accent));
                    let hint = match &self.view {
                        View::Notes => "Write a note…".to_string(),
                        View::Category(c) => format!("Add to {c}…"),
                        _ => "Add a task, note or reminder…   e.g. remind me to call Sam at 4pm".to_string(),
                    };
                    let te = egui::TextEdit::singleline(&mut self.input)
                        .hint_text(RichText::new(hint).color(pal.faint))
                        .frame(egui::Frame::NONE)
                        .font(egui::FontId::proportional(if big { 19.0 } else { 16.0 } * self.settings.font_scale))
                        .desired_width(f32::INFINITY)
                        .id(egui::Id::unique("main-input"));
                    let r = ui.add(te);
                    if self.focus_input {
                        r.request_focus();
                        self.focus_input = false;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        let ctx = ui.ctx().clone();
                        self.add_from_input(&ctx);
                        if self.mode == Mode::Full || !self.settings.close_capture_after_add {
                            self.focus_input = true;
                        }
                    }
                });
            });
        if !self.input.trim().is_empty() {
            let mut p = parse_with(&self.input, &self.settings);
            if self.view == View::Notes {
                p.kind = Kind::Note;
                p.remind = false;
            }
            let fallback = match &self.view {
                View::Category(c) => Some(c.clone()),
                _ => None,
            };
            let it = item_from(&p, &self.settings, fallback.as_deref());
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(if p.text.is_empty() { "…" } else { &p.text }).color(pal.dim).size(13.0));
            });
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                preview(ui, &p, &pal, &self.settings, &it.category);
            });
        }
    }

    fn rows(&mut self, ui: &mut egui::Ui, ids: &[u64], show_cat: bool) {
        let mut todo: Vec<(u64, RowAct)> = vec![];
        for &id in ids {
            let Some(it) = self.data.get(id).cloned() else { continue };
            let selected = self.editor.as_ref().is_some_and(|e| e.id == id);
            let a = item_row(ui, &it, &self.pal, &self.settings, &RowOpts { show_category: show_cat, selected, compact: self.settings.compact || self.mode == Mode::Capture });
            if a != RowAct::None {
                todo.push((id, a));
            }
            if self.confirm == Some(id) {
                ui.horizontal(|ui| {
                    ui.add_space(38.0);
                    ui.label(RichText::new("Delete this item?").color(self.pal.red));
                    if ui.button("Delete").clicked() {
                        todo.push((id, RowAct::Delete));
                    }
                    if ui.button("Keep").clicked() {
                        self.confirm = None;
                    }
                });
            }
        }
        for (id, a) in todo {
            if self.mode == Mode::Capture && a == RowAct::Open {
                // open the full app on that item
                self.view = View::Open;
                self.switch(Mode::Full);
            }
            self.act(id, a);
        }
    }

    fn editor_ui(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let cats = self.settings.categories.clone();
        let mut save = false;
        let mut cancel = false;
        let mut del = None;
        let s = self.settings.clone();
        let mut steps_changed = false;
        let Some(e) = self.editor.as_mut() else { return };
        egui::Frame::new()
            .fill(pal.card)
            .stroke(Stroke::new(1.0, pal.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .outer_margin(Margin { left: 0, right: 2, top: 0, bottom: 8 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let r = ui.add(egui::TextEdit::multiline(&mut e.text).desired_rows(2).desired_width(f32::INFINITY));
                if r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
                    e.text = e.text.trim_end_matches('\n').to_string();
                    save = true;
                }
                ui.add_space(4.0);
                let rows = if e.notes.is_empty() { 3 } else { 5 };
                ui.add(
                    egui::TextEdit::multiline(&mut e.notes)
                        .hint_text("Notes: details, links, anything (Enter for a new line)")
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY),
                );
                let links = crate::parse::links(&format!("{} {}", e.text, e.notes));
                if !links.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(icon(ic::LINK, 12.0, pal.dim));
                        for l in &links {
                            ui.hyperlink_to(RichText::new(crate::parse::link_label(l)).color(pal.blue), l).on_hover_text(l.as_str());
                        }
                    });
                }
                if e.kind == Kind::Task {
                    ui.add_space(6.0);
                    let n = e.steps.iter().filter(|x| x.done).count();
                    ui.label(RichText::new(if e.steps.is_empty() { "Steps".to_string() } else { format!("Steps  {n} of {}", e.steps.len()) }).color(pal.dim).size(12.0));
                    let mut remove = None;
                    let mut up = None;
                    for (k, st) in e.steps.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut st.done, "").changed() {
                                steps_changed = true;
                            }
                            let r = ui.add(egui::TextEdit::singleline(&mut st.text).desired_width((ui.available_width() - 70.0).max(120.0)).text_color(if st.done { pal.faint } else { pal.text }));
                            if r.lost_focus() {
                                steps_changed = true;
                            }
                            if ui.add_enabled(k > 0, egui::Button::new(icon("\u{E70E}", 11.0, pal.dim)).frame(false)).on_hover_text("Move up").clicked() {
                                up = Some(k);
                            }
                            if ui.add(egui::Button::new(icon(ic::DELETE, 11.0, pal.dim)).frame(false)).on_hover_text("Remove step").clicked() {
                                remove = Some(k);
                            }
                        });
                    }
                    if let Some(k) = up {
                        e.steps.swap(k, k - 1);
                        steps_changed = true;
                    }
                    if let Some(k) = remove {
                        e.steps.remove(k);
                        steps_changed = true;
                    }
                    ui.horizontal(|ui| {
                        ui.label(icon(ic::ADD, 12.0, pal.accent));
                        let r = ui.add(egui::TextEdit::singleline(&mut e.new_step).hint_text("Add a step (Enter)").desired_width((ui.available_width() - 70.0).max(120.0)));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if (enter || ui.button("Add").clicked()) && !e.new_step.trim().is_empty() {
                            e.steps.push(store::Step { text: e.new_step.trim().to_string(), done: false });
                            e.new_step.clear();
                            steps_changed = true;
                            r.request_focus();
                        }
                    });
                }
                ui.add_space(6.0);
                egui::Grid::new("ed").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(RichText::new("When").color(pal.dim));
                    ui.vertical(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut e.when).hint_text("tomorrow 4pm, fri, 25/12 9:30am…").desired_width(ui.available_width().min(260.0)));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            save = true;
                        }
                        let w = e.when.trim();
                        if !w.is_empty() {
                            let p = parse_with(&format!("x {}", fix_slash_time(w)), &s);
                            match p.due.and_then(|d| d.and_local_timezone(chrono::Local).single()) {
                                Some(d) => {
                                    ui.label(RichText::new(format!("→ {}", store::describe_due(d, p.all_day && !e.remind, s.use_24h))).color(pal.green));
                                }
                                None => {
                                    ui.label(RichText::new("not understood").color(pal.red));
                                }
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Pop-up").color(pal.dim));
                    ui.add_enabled(e.kind == Kind::Task, egui::Checkbox::new(&mut e.remind, "Remind me at that time"));
                    ui.end_row();
                    ui.label(RichText::new("Repeat").color(pal.dim));
                    ui.vertical(|ui| repeat_editor(ui, &pal, &mut e.repeat));
                    ui.end_row();
                    ui.label(RichText::new("Priority").color(pal.dim));
                    ui.horizontal(|ui| {
                        for p in Priority::ALL {
                            let txt = RichText::new(if p == Priority::None { "None" } else { p.label() }).color(if p == Priority::None { pal.text } else { priority_color(p, &pal) });
                            if ui.selectable_label(e.priority == p, txt).clicked() {
                                e.priority = p;
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Type").color(pal.dim));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut e.kind, Kind::Task, "Task");
                        ui.selectable_value(&mut e.kind, Kind::Note, "Note");
                    });
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Folder").color(pal.dim));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        for c in &cats {
                            let (g, col) = folder_look(&s, c);
                            let mut job = egui::text::LayoutJob::default();
                            job.append(g, 0.0, egui::TextFormat { font_id: icon_font(12.0), color: col.unwrap_or(pal.dim), valign: Align::Center, ..Default::default() });
                            job.append(c, 5.0, egui::TextFormat { font_id: egui::FontId::proportional(14.0 * s.font_scale), color: pal.text, valign: Align::Center, ..Default::default() });
                            if ui.selectable_label(&e.category == c, job).clicked() {
                                e.category = c.clone();
                            }
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Save").color(pal.on_accent)).fill(pal.accent)).clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(RichText::new(format!("{}  Delete", ic::DELETE)).color(pal.red)).clicked() {
                            del = Some(e.id);
                        }
                    });
                });
            });
        if steps_changed && let Some(e) = &self.editor {
            let steps: Vec<store::Step> = e.steps.iter().filter(|x| !x.text.trim().is_empty()).cloned().collect();
            store::set_steps(e.id, steps, true);
            self.reload();
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        }
        if save {
            self.save_editor();
        } else if cancel {
            self.editor = None;
        }
        if let Some(id) = del {
            self.editor = None;
            self.act(id, RowAct::Delete);
        }
    }

    /// The details pane on the right: the open item's editor.
    fn detail_pane(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        egui::Frame::new().fill(pal.panel).inner_margin(Margin { left: 14, right: 12, top: 12, bottom: 8 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            let mut close = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Details").font(semi(15.0)).color(pal.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let b = ui.add(egui::Button::new(icon(ic::CLOSE, 10.0, pal.dim)).frame(false)).on_hover_text("Close (Esc)");
                    if b.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        close = true;
                    }
                });
            });
            if let Some(it) = self.editor.as_ref().and_then(|e| self.data.get(e.id)) {
                let f = if self.settings.use_24h { "%a %-d %b %Y, %H:%M" } else { "%a %-d %b %Y, %-I:%M%P" };
                let mut meta = format!("Added {}", it.created.format(f));
                if let Some(d) = it.done_at.filter(|_| it.done) {
                    meta += &format!("   Done {}", d.format(f));
                }
                if !it.source.is_empty() {
                    meta += &format!("   From {}", it.source);
                }
                ui.label(RichText::new(meta).size(11.5).color(pal.faint));
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.editor_ui(ui));
            if close {
                self.editor = None;
            }
        });
    }

    fn visible_done(&self, i: &Item) -> bool {
        let keep = chrono::Duration::days(self.settings.show_done_days as i64);
        i.done && i.done_at.is_none_or(|d| chrono::Local::now() - d < keep)
    }

    fn matches_search(&self, i: &Item) -> bool {
        let q = self.search.trim().to_lowercase();
        q.is_empty() || i.text.to_lowercase().contains(&q) || i.category.to_lowercase().contains(&q) || i.tags.iter().any(|t| t.to_lowercase().contains(&q))
    }

    fn open_ids(&self, f: impl Fn(&Item) -> bool) -> Vec<u64> {
        let mut v: Vec<&Item> = self.data.items.iter().filter(|i| !i.done && f(i) && self.matches_search(i)).collect();
        sort_open(&mut v);
        v.iter().map(|i| i.id).collect()
    }

    // ------------------------------------------------------------ capture

    /// Returns the height the content needs, so the window can fit it.
    fn capture_ui(&mut self, ui: &mut egui::Ui) -> f32 {
        let pal = self.pal;
        let mut need = 0.0;
        egui::Frame::new().fill(pal.bg).inner_margin(Margin::same(12)).show(ui, |ui| {
            let top = ui.cursor().top();
            ui.set_width(ui.available_width());
            self.input_box(ui, true);
            if self.settings.capture_shows_list {
                let ids: Vec<u64> = self.open_ids(|i| i.kind == Kind::Task).into_iter().take(self.settings.capture_list_size).collect();
                let total = self.data.items.iter().filter(|i| !i.done && i.kind == Kind::Task).count();
                if !ids.is_empty() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new("UP NEXT").size(11.0).color(pal.faint).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if total > ids.len() {
                                ui.label(RichText::new(format!("{} more", total - ids.len())).size(11.0).color(pal.faint));
                            }
                        });
                    });
                    self.rows(ui, &ids, true);
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let k = |ui: &mut egui::Ui, key: &str, what: &str| {
                    ui.label(RichText::new(key).size(11.0).color(pal.dim).background_color(pal.card));
                    ui.label(RichText::new(what).size(11.0).color(pal.faint));
                    ui.add_space(6.0);
                };
                k(ui, " Enter ", "add");
                k(ui, " Esc ", "close");
                k(ui, " Ctrl+O ", "open Sticky");
                k(ui, " ! !! !!! ", "priority");
                k(ui, " #later ", "folder");
            });
            if let Some((m, t)) = &self.flash
                && t.elapsed().as_secs_f32() < 2.0
            {
                ui.label(RichText::new(format!("{}  {m}", ic::CHECK)).color(pal.green).size(12.0));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
            need = ui.cursor().top() - top;
        });
        need + 24.0
    }

    // ------------------------------------------------------------ full

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        // The drag area goes in first: egui gives clicks to whatever was added last, so the
        // window buttons drawn afterwards sit on top of it.
        let avail = ui.available_rect_before_wrap();
        let bar_rect = egui::Rect::from_min_size(avail.min, Vec2::new(avail.width(), 38.0));
        let r = ui.interact(bar_rect, egui::Id::unique("titlebar"), egui::Sense::click_and_drag());
        if r.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if r.double_clicked() {
            let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maxed));
        }
        egui::Frame::new().fill(pal.panel).inner_margin(Margin { left: 14, right: 0, top: 0, bottom: 0 }).show(ui, |ui| {
            ui.set_height(38.0);
            ui.horizontal_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                ui.painter().rect_filled(r.shrink(1.0), CornerRadius::same(4), rgbish(0xFFC83D));
                ui.label(RichText::new("Sticky").font(semi(14.0)).color(pal.text));
                let open = self.data.items.iter().filter(|i| !i.done && i.kind == Kind::Task).count();
                ui.label(RichText::new(format!("{open} open")).size(12.0).color(pal.faint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    for (g, what) in [(ic::CLOSE, 0), (if maxed { ic::RESTORE } else { ic::MAX }, 1), (ic::MIN, 2)] {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(46.0, 38.0), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::ZERO, if what == 0 { Color32::from_rgb(196, 43, 28) } else { pal.hover });
                        }
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, g, icon_font(10.0), if resp.hovered() && what == 0 { Color32::WHITE } else { pal.dim });
                        if resp.clicked() {
                            match what {
                                0 => ctx.send_viewport_cmd(ViewportCommand::Close),
                                1 => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maxed)),
                                _ => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                            }
                        }
                    }
                });
            });
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        egui::Frame::new().fill(pal.panel).inner_margin(Margin { left: 10, right: 10, top: 4, bottom: 10 }).show(ui, |ui| {
            ui.set_width(196.0);
            ui.set_height(ui.available_height());
            egui::Frame::new().fill(pal.card).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(10, 5)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(icon(ic::SEARCH, 12.0, pal.faint));
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").frame(egui::Frame::NONE).desired_width(f32::INFINITY));
                });
            });
            ui.add_space(10.0);
            let open_tasks = |f: &dyn Fn(&Item) -> bool| self.data.items.iter().filter(|i| !i.done && f(i)).count();
            let mut entries: Vec<(String, &str, View, usize)> = vec![("All open".into(), ic::INBOX, View::Open, open_tasks(&|i| i.kind == Kind::Task))];
            for c in &self.settings.categories {
                let cc = c.clone();
                entries.push((c.clone(), folder_look(&self.settings, c).0, View::Category(c.clone()), open_tasks(&move |i: &Item| i.kind == Kind::Task && i.category == cc)));
            }
            let other = open_tasks(&|i| i.kind == Kind::Task && !self.settings.categories.contains(&i.category));
            if other > 0 {
                entries.push(("Other".into(), ic::FOLDER, View::Category(String::new()), other));
            }
            entries.push(("Reminders".into(), ic::BELL, View::Reminders, open_tasks(&|i| i.remind)));
            entries.push(("Notes".into(), ic::NOTE, View::Notes, open_tasks(&|i| i.kind == Kind::Note)));
            let done = self.data.items.iter().filter(|i| self.visible_done(i)).count();
            entries.push(("Done".into(), ic::DONE_ALL, View::Done, done));
            let mut clicked = None;
            let mut folder_act: Option<FolderAct> = None;
            let folders = self.settings.categories.clone();
            for (idx, (label, g, v, n)) in entries.into_iter().enumerate() {
                if idx == 1 {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("FOLDERS").size(10.5).color(pal.faint));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let b = ui.add(egui::Button::new(icon(ic::ADD, 11.0, pal.dim)).frame(false)).on_hover_text("New folder");
                            if b.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                self.new_folder = Some(String::new());
                                self.renaming = None;
                            }
                        });
                    });
                }
                if label == "Reminders" {
                    if let Some(name) = self.new_folder.as_mut() {
                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            ui.label(icon(ic::FOLDER, 13.0, pal.accent));
                            let r = ui.add(egui::TextEdit::singleline(name).hint_text("Folder name").desired_width(f32::INFINITY));
                            if !r.has_focus() && !r.lost_focus() {
                                r.request_focus();
                            }
                            if r.lost_focus() {
                                folder_act = Some(if ui.input(|i| i.key_pressed(Key::Enter)) { FolderAct::Add(name.trim().to_string()) } else { FolderAct::CancelNew });
                            }
                        });
                        ui.label(RichText::new("Enter to add, Esc to cancel").size(11.0).color(pal.faint));
                    }
                    ui.add_space(8.0);
                }
                let is_folder = matches!(&v, View::Category(c) if !c.is_empty());
                if is_folder && self.renaming.as_ref().is_some_and(|(old, _)| *old == label) {
                    let (_, new) = self.renaming.as_mut().unwrap();
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(icon(ic::FOLDER, 13.0, pal.accent));
                        let r = ui.add(egui::TextEdit::singleline(new).desired_width(f32::INFINITY));
                        if !r.has_focus() && !r.lost_focus() {
                            r.request_focus();
                        }
                        if r.lost_focus() {
                            folder_act = Some(if ui.input(|i| i.key_pressed(Key::Escape)) { FolderAct::CancelRename } else { FolderAct::Rename(label.clone(), new.trim().to_string()) });
                        }
                    });
                    continue;
                }
                let tint = if is_folder { folder_look(&self.settings, &label).1 } else { None };
                let r = nav_resp(ui, &pal, g, &label, Some(n), self.view == v, tint);
                if r.clicked() {
                    clicked = Some(v.clone());
                }
                if is_folder {
                    let pos = folders.iter().position(|f| *f == label).unwrap_or(0);
                    r.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            folder_act = Some(FolderAct::StartRename(label.clone()));
                            ui.close();
                        }
                        if ui.add_enabled(pos > 0, egui::Button::new("Move up")).clicked() {
                            folder_act = Some(FolderAct::Move(pos, pos - 1));
                            ui.close();
                        }
                        if ui.add_enabled(pos + 1 < folders.len(), egui::Button::new("Move down")).clicked() {
                            folder_act = Some(FolderAct::Move(pos, pos + 1));
                            ui.close();
                        }
                        ui.separator();
                        let style = self.settings.folder_styles.get(&label).cloned().unwrap_or_default();
                        ui.menu_button("Colour", |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (name, hx) in FOLDER_COLOURS {
                                    let c = hex(hx).unwrap_or(pal.dim);
                                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::click());
                                    ui.painter().circle_filled(r.center(), 8.0, c);
                                    if style.color.eq_ignore_ascii_case(hx) || resp.hovered() {
                                        ui.painter().circle_stroke(r.center(), 10.5, Stroke::new(1.5, pal.text));
                                    }
                                    if resp.on_hover_text(*name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        folder_act = Some(FolderAct::Colour(label.clone(), hx.to_string()));
                                        ui.close();
                                    }
                                }
                            });
                            if ui.add_enabled(!style.color.is_empty(), egui::Button::new("No colour")).clicked() {
                                folder_act = Some(FolderAct::Colour(label.clone(), String::new()));
                                ui.close();
                            }
                        });
                        ui.menu_button("Icon", |ui| {
                            let tint = hex(&style.color).unwrap_or(pal.text);
                            egui::Grid::new("folder-icons").spacing([2.0, 2.0]).show(ui, |ui| {
                                for (k, (name, g)) in FOLDER_ICONS.iter().enumerate() {
                                    let sel = style.icon == *name || (style.icon.is_empty() && k == 0);
                                    let b = ui.add(egui::Button::new(icon(g, 16.0, tint)).selected(sel).min_size(Vec2::splat(30.0))).on_hover_text(*name);
                                    if b.clicked() {
                                        folder_act = Some(FolderAct::Icon(label.clone(), if k == 0 { String::new() } else { name.to_string() }));
                                        ui.close();
                                    }
                                    if k % 8 == 7 {
                                        ui.end_row();
                                    }
                                }
                            });
                        });
                        ui.separator();
                        let tip = if n > 0 { format!("Its {n} open item(s) move to {}", self.settings.default_category) } else { "It's empty".to_string() };
                        if ui.add_enabled(folders.len() > 1, egui::Button::new(RichText::new("Delete folder").color(pal.red))).on_hover_text(tip).clicked() {
                            folder_act = Some(FolderAct::Delete(label.clone()));
                            ui.close();
                        }
                    });
                }
            }
            if let Some(a) = folder_act {
                self.folder_action(a);
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                if nav(ui, &pal, ic::SETTINGS, "Settings", None, self.view == View::Settings) {
                    clicked = Some(View::Settings);
                }
                if nav(ui, &pal, ic::HISTORY, "Activity log", None, self.view == View::Log) {
                    clicked = Some(View::Log);
                }
            });
            if let Some(v) = clicked {
                self.view = v;
                self.editor = None;
                if self.view == View::Log {
                    self.log = store::read_log(2000);
                }
                self.focus_input = matches!(self.view, View::Open | View::Category(_) | View::Notes);
            }
        });
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        egui::Frame::new().fill(pal.bg).inner_margin(Margin { left: 22, right: 18, top: 14, bottom: 8 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            let title = match &self.view {
                View::Open => "All open".to_string(),
                View::Category(c) if c.is_empty() => "Other".to_string(),
                View::Category(c) => c.clone(),
                View::Reminders => "Reminders".into(),
                View::Notes => "Notes".into(),
                View::Done => "Done".into(),
                View::Log => "Activity log".into(),
                View::Settings => "Settings".into(),
            };
            ui.horizontal(|ui| {
                if let View::Category(c) = &self.view
                    && !c.is_empty()
                {
                    let (g, col) = folder_look(&self.settings, c);
                    ui.label(icon(g, 18.0, col.unwrap_or(pal.dim)));
                }
                ui.heading(&title);
                if self.view == View::Done {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Clear done").on_hover_text("Delete every done item (they stay in the log)").clicked() {
                            let n = store::update(|d| {
                                let before = d.items.len();
                                d.items.retain(|i| !i.done);
                                before - d.items.len()
                            });
                            store::log(&format!("cleared {n} done"), 0, "");
                            self.reload();
                        }
                    });
                }
            });
            ui.add_space(8.0);
            match self.view.clone() {
                View::Settings => {
                    super::settings_view::show(self, ui);
                    return;
                }
                View::Log => {
                    self.log_ui(ui);
                    return;
                }
                _ => {}
            }
            if self.view != View::Done {
                self.input_box(ui, false);
                ui.add_space(10.0);
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.view.clone() {
                    View::Open => {
                        let mut any = false;
                        let mut cats = self.settings.categories.clone();
                        cats.push(String::new());
                        for c in cats {
                            let known = self.settings.categories.clone();
                            let ids = self.open_ids(|i| i.kind == Kind::Task && if c.is_empty() { !known.contains(&i.category) } else { i.category == c });
                            if ids.is_empty() {
                                continue;
                            }
                            any = true;
                            let (g, col) = folder_look(&self.settings, &c);
                            folder_header(ui, &pal, g, col, if c.is_empty() { "Other" } else { &c }, ids.len());
                            self.rows(ui, &ids, false);
                            ui.add_space(8.0);
                        }
                        if !any {
                            empty(ui, &pal, "Nothing open. Type above to add something.");
                        }
                    }
                    View::Category(c) => {
                        let known = self.settings.categories.clone();
                        let ids = self.open_ids(|i| i.kind == Kind::Task && if c.is_empty() { !known.contains(&i.category) } else { i.category == c });
                        if ids.is_empty() {
                            empty(ui, &pal, "Nothing here.");
                        }
                        self.rows(ui, &ids, false);
                    }
                    View::Reminders => {
                        let mut v: Vec<&Item> = self.data.items.iter().filter(|i| !i.done && i.due.is_some() && self.matches_search(i)).collect();
                        v.sort_by_key(|i| i.snoozed_until.or(i.due));
                        let ids: Vec<u64> = v.iter().map(|i| i.id).collect();
                        if ids.is_empty() {
                            empty(ui, &pal, "No reminders. Try \"remind me to stretch in 30 min\".");
                        }
                        self.rows(ui, &ids, true);
                    }
                    View::Notes => {
                        let ids = self.open_ids(|i| i.kind == Kind::Note);
                        if ids.is_empty() {
                            empty(ui, &pal, "No notes. Start with \"note:\" or type above.");
                        }
                        self.rows(ui, &ids, true);
                    }
                    View::Done => {
                        let mut v: Vec<&Item> = self.data.items.iter().filter(|i| i.done && self.matches_search(i)).collect();
                        v.sort_by(|a, b| b.done_at.cmp(&a.done_at));
                        let ids: Vec<u64> = v.iter().map(|i| i.id).collect();
                        if ids.is_empty() {
                            empty(ui, &pal, "Nothing done yet.");
                        }
                        self.rows(ui, &ids, true);
                    }
                    _ => {}
                }
            });
        });
    }

    fn log_ui(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let q = self.search.trim().to_lowercase();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut last_day = None;
            let mut shown = 0;
            for e in &self.log {
                if !q.is_empty() && !e.text.to_lowercase().contains(&q) && !e.action.contains(&q) {
                    continue;
                }
                let day = e.at.date_naive();
                if last_day != Some(day) {
                    ui.add_space(8.0);
                    group_header(ui, &pal, &store::day_word(day, chrono::Local::now().date_naive()), 0);
                    last_day = Some(day);
                }
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    let t = if self.settings.use_24h { e.at.format("%H:%M").to_string() } else { e.at.format("%-I:%M %P").to_string() };
                    ui.add_sized([64.0, 20.0], egui::Label::new(RichText::new(t).color(pal.faint).size(12.5)));
                    let c = match e.action.as_str() {
                        a if a.starts_with("done") => pal.green,
                        "added" => pal.accent,
                        "reminded" => pal.amber,
                        a if a.starts_with("snoozed") => pal.amber,
                        "deleted" => pal.red,
                        a if a.contains("failed") || a.contains("invalid") => pal.red,
                        _ => pal.dim,
                    };
                    chip(ui, "", &e.action, c);
                    ui.label(&e.text);
                });
                shown += 1;
            }
            if shown == 0 {
                empty(ui, &pal, "No activity yet.");
            }
        });
    }
}

fn rgbish(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The "When" box: a bare hour like "4" means a time there, so read it as "at 4".
fn fix_slash_time(s: &str) -> String {
    let w: Vec<&str> = s.split_whitespace().collect();
    w.iter()
        .enumerate()
        .map(|(i, t)| {
            let prev_at = i > 0 && matches!(w[i - 1], "at" | "@");
            let next_ok = w.get(i + 1).is_none_or(|n| matches!(*n, "am" | "pm"));
            if !prev_at && next_ok && t.parse::<u32>().is_ok_and(|h| (1..=23).contains(&h)) { format!("at {t}") } else { t.to_string() }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn nav(ui: &mut egui::Ui, pal: &Pal, g: &str, label: &str, count: Option<usize>, sel: bool) -> bool {
    nav_resp(ui, pal, g, label, count, sel, None).clicked()
}

/// A sidebar entry. `tint` is a folder's own colour for its icon.
fn nav_resp(ui: &mut egui::Ui, pal: &Pal, g: &str, label: &str, count: Option<usize>, sel: bool, tint: Option<Color32>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), egui::Sense::click());
    let fill = if sel { pal.accent.gamma_multiply(0.16) } else if resp.hovered() { pal.hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(r, CornerRadius::same(7), fill);
    if sel {
        ui.painter().rect_filled(egui::Rect::from_min_size(r.left_top() + Vec2::new(0.0, 8.0), Vec2::new(3.0, 16.0)), CornerRadius::same(2), pal.accent);
    }
    let c = if sel { pal.text } else { pal.dim };
    ui.painter().text(r.left_center() + Vec2::new(14.0, 0.0), egui::Align2::LEFT_CENTER, g, icon_font(13.0), tint.unwrap_or(if sel { pal.accent } else { pal.faint }));
    ui.painter().text(r.left_center() + Vec2::new(38.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), c);
    if let Some(n) = count.filter(|n| *n > 0) {
        ui.painter().text(r.right_center() - Vec2::new(10.0, 0.0), egui::Align2::RIGHT_CENTER, n.to_string(), egui::FontId::proportional(12.0), pal.faint);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

enum FolderAct {
    Add(String),
    CancelNew,
    StartRename(String),
    Rename(String, String),
    CancelRename,
    Move(usize, usize),
    Delete(String),
    Colour(String, String),
    Icon(String, String),
}

fn group_header(ui: &mut egui::Ui, pal: &Pal, t: &str, n: usize) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(t.to_uppercase()).size(11.0).color(pal.faint).strong());
        if n > 0 {
            ui.label(RichText::new(n.to_string()).size(11.0).color(pal.faint));
        }
    });
}

fn folder_header(ui: &mut egui::Ui, pal: &Pal, g: &str, col: Option<Color32>, t: &str, n: usize) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(icon(g, 11.0, col.unwrap_or(pal.faint)));
        ui.label(RichText::new(t.to_uppercase()).size(11.0).color(col.unwrap_or(pal.faint)).strong());
        if n > 0 {
            ui.label(RichText::new(n.to_string()).size(11.0).color(pal.faint));
        }
    });
}

fn empty(ui: &mut egui::Ui, pal: &Pal, t: &str) {
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(t).color(pal.faint));
    });
}

impl eframe::App for MainApp {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        self.pal.bg.to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.watch.items.swap(false, Ordering::SeqCst) {
            self.reload();
        }
        if self.watch.settings.swap(false, Ordering::SeqCst) {
            self.settings = Settings::load();
            self.pal = palette(&self.settings);
            apply_style(ctx, &self.pal, &self.settings);
        }
        let req = self.watch.request.lock().unwrap().take();
        if let Some(r) = req {
            match r.as_str() {
                "capture" => {
                    if self.mode == Mode::Full {
                        self.view = View::Open;
                    }
                    self.focus_input = true;
                }
                "settings" => {
                    self.view = View::Settings;
                    self.switch(Mode::Full);
                }
                "log" => {
                    self.view = View::Log;
                    self.log = store::read_log(2000);
                    self.switch(Mode::Full);
                }
                _ => self.switch(Mode::Full),
            }
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            if let Some(h) = self.hwnd {
                win::force_foreground(h);
            }
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        if self.frames == 2
            && std::env::var_os("STICKY_NO_FOCUS").is_none()
            && let Some(h) = self.hwnd
        {
            win::force_foreground(h);
        }
        // keys
        let (esc, ctrl_o, ctrl_comma, ctrl_f) = ctx.input(|i| {
            (
                i.key_pressed(Key::Escape),
                i.modifiers.command && i.key_pressed(Key::O),
                i.modifiers.command && i.key_pressed(Key::Comma),
                i.modifiers.command && i.key_pressed(Key::F),
            )
        });
        if ctrl_o && self.mode == Mode::Capture {
            self.switch(Mode::Full);
        }
        if ctrl_comma {
            self.view = View::Settings;
            self.switch(Mode::Full);
        }
        if ctrl_f && self.mode == Mode::Full {
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("main-input")));
        }
        if esc && self.editor.is_none() {
            if !self.input.is_empty() {
                self.input.clear();
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        // quick add closes when you click away, like a launcher
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if self.mode == Mode::Capture {
            if focused {
                self.was_focused = true;
            } else if self.was_focused && self.input.trim().is_empty() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }

        match self.mode {
            Mode::Capture => {
                // Drag anywhere on the background. Added before the content: egui gives the
                // pointer to whatever was added last, so buttons and rows go on top of it.
                let bg = ui.interact(ctx.content_rect(), egui::Id::unique("bgdrag"), egui::Sense::drag());
                if bg.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                let r = egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ui, |ui| {
                    self.capture_ui(ui)
                });
                let h = r.inner.ceil().clamp(80.0, 900.0);
                if (h - self.last_height).abs() > 1.0 {
                    self.last_height = h;
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(CAPTURE_W, h)));
                }
            }
            Mode::Full => {
                egui::Panel::top("title").frame(egui::Frame::NONE).show(ui, |ui| self.title_bar(ui));
                egui::Panel::left("side").frame(egui::Frame::NONE).resizable(false).exact_size(216.0).show(ui, |ui| self.sidebar(ui));
                let lists = matches!(self.view, View::Open | View::Category(_) | View::Reminders | View::Notes | View::Done);
                if lists && self.editor.as_ref().is_some_and(|e| self.data.get(e.id).is_some()) {
                    let r = egui::Panel::right("details")
                        .frame(egui::Frame::NONE)
                        .resizable(true)
                        .default_size(self.settings.detail_width)
                        .size_range(300.0..=760.0)
                        .show(ui, |ui| self.detail_pane(ui));
                    let w = r.response.rect.width().round();
                    if (w - self.settings.detail_width).abs() > 3.0 && ctx.input(|i| !i.pointer.any_down()) && w >= 300.0 {
                        self.settings.detail_width = w;
                        let mut disk = Settings::load();
                        disk.detail_width = w;
                        disk.save();
                    }
                }
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.content(ui));
                resize_edges(ui);
                // remember the size
                let size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
                let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if let Some(s) = size
                    && !maxed
                    && ((s.x - self.settings.main_size[0]).abs() > 4.0 || (s.y - self.settings.main_size[1]).abs() > 4.0)
                    && s.x >= 600.0
                    && s.y >= 400.0
                    && ctx.input(|i| !i.pointer.any_down())
                {
                    self.settings.main_size = [s.x, s.y];
                    let mut disk = Settings::load();
                    disk.main_size = [s.x, s.y];
                    disk.save();
                }
            }
        }
    }
}

/// Never, the To Do presets, or Custom: every N minutes/hours/days/weeks/months/years, which days,
/// and for minutes/hours an optional time window.
fn repeat_editor(ui: &mut egui::Ui, pal: &Pal, rep: &mut Option<Repeat>) {
    use crate::repeat::{Unit, time_label};
    let cap = |s: String| s[..1].to_uppercase() + &s[1..];
    let custom = rep.is_some_and(|r| r.is_custom());
    let shown = match *rep {
        None => "Never".to_string(),
        Some(r) if r.is_custom() => "Custom".to_string(),
        Some(r) => cap(r.label()),
    };
    egui::ComboBox::from_id_salt("rep").selected_text(shown).width(170.0).show_ui(ui, |ui| {
        if ui.selectable_label(rep.is_none(), "Never").clicked() {
            *rep = None;
        }
        for r in Repeat::PRESETS {
            if ui.selectable_label(*rep == Some(r), cap(r.label())).clicked() {
                *rep = Some(r);
            }
        }
        ui.separator();
        if ui.selectable_label(custom, "Custom…").clicked() && !custom {
            *rep = Some(rep.map(|r| r.as_custom()).unwrap_or(Repeat::every(1, Unit::Days)));
        }
    });
    let Some(Repeat::Every { mut n, mut unit, mut days, mut window }) = *rep else { return };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Every");
        ui.add(egui::DragValue::new(&mut n).range(1..=999).speed(0.2));
        egui::ComboBox::from_id_salt("rep-unit").selected_text(if n == 1 { unit.singular() } else { unit.plural() }).width(90.0).show_ui(ui, |ui| {
            for u in Unit::ALL {
                ui.selectable_value(&mut unit, u, if n == 1 { u.singular() } else { u.plural() });
            }
        });
    });
    if unit == Unit::Weeks || unit.intraday() {
        ui.horizontal(|ui| {
            ui.label(if unit == Unit::Weeks { "On" } else { "Only on" });
            for (i, l) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
                let on = days & (1 << i) != 0;
                let txt = RichText::new(*l).color(if on { pal.on_accent } else { pal.dim });
                let b = egui::Button::new(txt).fill(if on { pal.accent } else { pal.card }).min_size(egui::vec2(26.0, 24.0));
                if ui.add(b).on_hover_text(["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][i]).clicked() {
                    days ^= 1 << i;
                }
            }
            if days == 0 && unit.intraday() {
                ui.label(RichText::new("every day").color(pal.faint).size(12.0));
            }
        });
    } else {
        days = 0;
    }
    if unit.intraday() {
        ui.horizontal(|ui| {
            let mut on = window.is_some();
            if ui.checkbox(&mut on, "Only between").changed() {
                window = on.then_some((9 * 60, 17 * 60));
            }
            if let Some((a, b)) = &mut window {
                let pick = |ui: &mut egui::Ui, id: &str, v: &mut u16| {
                    egui::ComboBox::from_id_salt(id).selected_text(time_label(*v)).width(80.0).height(260.0).show_ui(ui, |ui| {
                        for m in (0..24 * 60).step_by(30) {
                            ui.selectable_value(v, m as u16, time_label(m as u16));
                        }
                        ui.selectable_value(v, 23 * 60 + 59, time_label(23 * 60 + 59));
                    });
                };
                pick(ui, "rep-from", a);
                ui.label("and");
                pick(ui, "rep-to", b);
                if *b <= *a {
                    ui.label(RichText::new("end is before start").color(pal.red).size(12.0));
                }
            }
        });
    } else {
        window = None;
    }
    let r = Repeat::Every { n, unit, days, window };
    *rep = Some(r);
    ui.label(RichText::new(format!("Repeats {}", r.label())).color(pal.faint).size(12.0));
}