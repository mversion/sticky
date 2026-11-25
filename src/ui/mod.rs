//! Shared look: palette, fonts, icons, the item row, and a file watcher that wakes the UI.

pub mod main_window;
pub mod note;
pub mod popup;
pub mod settings_view;
pub mod setup;
pub mod toast;

use crate::settings::Settings;
use crate::store::{self, Item, Kind, Priority};
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Sense, Stroke, TextStyle, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Segoe Fluent Icons code points.
pub mod ic {
    pub const CHECK: &str = "\u{E73E}";
    pub const ADD: &str = "\u{E710}";
    pub const CLOCK: &str = "\u{E823}";
    pub const BELL: &str = "\u{EA8F}";
    pub const SETTINGS: &str = "\u{E713}";
    pub const SEARCH: &str = "\u{E721}";
    pub const PIN: &str = "\u{E718}";
    pub const DELETE: &str = "\u{E74D}";
    pub const CLOSE: &str = "\u{E8BB}";
    pub const MIN: &str = "\u{E921}";
    pub const MAX: &str = "\u{E922}";
    pub const RESTORE: &str = "\u{E923}";
    pub const FLAG: &str = "\u{E7C1}";
    pub const HISTORY: &str = "\u{E81C}";
    pub const LIST: &str = "\u{E8FD}";
    pub const NOTE: &str = "\u{E70B}";
    pub const REPEAT: &str = "\u{E8EE}";
    pub const CALENDAR: &str = "\u{E787}";
    pub const EXPAND: &str = "\u{E740}";
    pub const FOLDER: &str = "\u{E8B7}";
    pub const DONE_ALL: &str = "\u{E930}";
    pub const INBOX: &str = "\u{E71D}";
    pub const SNOOZE: &str = "\u{E916}";
}

#[derive(Clone, Copy, Debug)]
pub struct Pal {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub red: Color32,
    pub amber: Color32,
    pub green: Color32,
    pub blue: Color32,
}

pub fn hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok()?;
    match s.len() {
        6 => Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)),
        _ => None,
    }
}

fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const THEMES: [(&str, &str); 5] =
    [("system", "Follow Windows"), ("midnight", "Midnight"), ("graphite", "Graphite"), ("paper", "Paper"), ("sticky", "Sticky yellow")];

pub fn palette(s: &Settings) -> Pal {
    let theme = match s.theme.as_str() {
        "system" => {
            if crate::win::system_light() {
                "paper"
            } else {
                "midnight"
            }
        }
        t => t,
    };
    let accent = hex(&s.accent).unwrap_or(rgb(0xFFC83D));
    let mut p = match theme {
        "graphite" => Pal {
            dark: true,
            bg: rgb(0x1C1C1C),
            panel: rgb(0x232323),
            card: rgb(0x2B2B2B),
            hover: rgb(0x333333),
            border: rgb(0x3A3A3A),
            text: rgb(0xF0F0F0),
            dim: rgb(0xA0A0A0),
            faint: rgb(0x6A6A6A),
            accent,
            on_accent: Color32::BLACK,
            red: rgb(0xFF6B6B),
            amber: rgb(0xFFB547),
            green: rgb(0x5BD08A),
            blue: rgb(0x6CB4FF),
        },
        "paper" => Pal {
            dark: false,
            bg: rgb(0xF6F5F2),
            panel: rgb(0xEDEBE6),
            card: rgb(0xFFFFFF),
            hover: rgb(0xF0EEE9),
            border: rgb(0xDDD9D0),
            text: rgb(0x1F1D1A),
            dim: rgb(0x6B665E),
            faint: rgb(0xA39E95),
            accent,
            on_accent: Color32::BLACK,
            red: rgb(0xD64545),
            amber: rgb(0xC98200),
            green: rgb(0x2E9E5E),
            blue: rgb(0x2F7BD8),
        },
        "sticky" => Pal {
            dark: false,
            bg: rgb(0xFFF4B0),
            panel: rgb(0xFBE98A),
            card: rgb(0xFFF9D6),
            hover: rgb(0xFFF0A0),
            border: rgb(0xE9D66A),
            text: rgb(0x3B3000),
            dim: rgb(0x7A6A20),
            faint: rgb(0xAE9F50),
            accent: if s.accent.eq_ignore_ascii_case("#FFC83D") { rgb(0xC2185B) } else { accent },
            on_accent: Color32::WHITE,
            red: rgb(0xC62828),
            amber: rgb(0xB26A00),
            green: rgb(0x2E7D32),
            blue: rgb(0x1565C0),
        },
        _ => Pal {
            dark: true,
            bg: rgb(0x15171C),
            panel: rgb(0x1B1E24),
            card: rgb(0x21252D),
            hover: rgb(0x2A2F38),
            border: rgb(0x30353F),
            text: rgb(0xE9ECF1),
            dim: rgb(0x9AA2AF),
            faint: rgb(0x626A77),
            accent,
            on_accent: Color32::BLACK,
            red: rgb(0xFF6B6B),
            amber: rgb(0xFFB547),
            green: rgb(0x5BD08A),
            blue: rgb(0x6CB4FF),
        },
    };
    // readable text on the accent
    let a = p.accent;
    let lum = 0.299 * a.r() as f32 + 0.587 * a.g() as f32 + 0.114 * a.b() as f32;
    p.on_accent = if lum > 150.0 { Color32::from_rgb(20, 20, 20) } else { Color32::WHITE };
    p
}

pub fn priority_color(p: Priority, pal: &Pal) -> Color32 {
    match p {
        Priority::High => pal.red,
        Priority::Medium => pal.amber,
        Priority::Low => pal.blue,
        Priority::None => pal.faint,
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = std::path::Path::new("C:\\Windows\\Fonts");
    let load = |names: &[&str]| names.iter().find_map(|n| std::fs::read(dir.join(n)).ok());
    let mut regular = vec![];
    if let Some(b) = load(&["segoeui.ttf"]) {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(b)));
        regular.push("segoe".to_string());
    }
    let mut semi = vec![];
    if let Some(b) = load(&["seguisb.ttf", "segoeuib.ttf"]) {
        fonts.font_data.insert("segoe-semi".into(), Arc::new(FontData::from_owned(b)));
        semi.push("segoe-semi".to_string());
    }
    let mut icons = vec![];
    if let Some(b) = load(&["SegoeIcons.ttf", "segmdl2.ttf"]) {
        fonts.font_data.insert("icons".into(), Arc::new(FontData::from_owned(b)));
        icons.push("icons".to_string());
    }
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let prop: Vec<String> = regular.iter().chain(icons.iter()).chain(defaults.iter()).cloned().collect();
    let semi_f: Vec<String> = semi.iter().chain(regular.iter()).chain(icons.iter()).chain(defaults.iter()).cloned().collect();
    let icon_f: Vec<String> = icons.iter().chain(defaults.iter()).cloned().collect();
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name("semi".into()), semi_f);
    fonts.families.insert(FontFamily::Name("icons".into()), icon_f);
    ctx.set_fonts(fonts);
}

pub fn semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semi".into()))
}
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}
pub fn icon(s: &str, size: f32, c: Color32) -> RichText {
    RichText::new(s).font(icon_font(size)).color(c)
}

pub fn apply_style(ctx: &egui::Context, pal: &Pal, s: &Settings) {
    let theme = if pal.dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(theme);
    let k = s.font_scale.clamp(0.7, 1.6);
    ctx.style_mut_of(theme, |st| {
        use TextStyle::*;
        st.text_styles = [
            (Small, FontId::proportional(12.0 * k)),
            (Body, FontId::proportional(14.5 * k)),
            (Button, FontId::proportional(14.0 * k)),
            (Heading, semi(21.0 * k)),
            (Monospace, FontId::monospace(13.0 * k)),
        ]
        .into();
        st.spacing.item_spacing = Vec2::new(8.0, if s.compact { 4.0 } else { 6.0 });
        st.spacing.button_padding = Vec2::new(10.0, 5.0);
        st.spacing.interact_size.y = 26.0;
        let v = &mut st.visuals;
        v.dark_mode = pal.dark;
        v.override_text_color = Some(pal.text);
        v.panel_fill = pal.bg;
        v.window_fill = pal.panel;
        v.extreme_bg_color = pal.card;
        v.faint_bg_color = pal.panel;
        v.hyperlink_color = pal.accent;
        v.selection.bg_fill = pal.accent.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, pal.accent);
        v.text_cursor.stroke = Stroke::new(2.0, pal.accent);
        v.window_stroke = Stroke::new(1.0, pal.border);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        let w = &mut v.widgets;
        for (st, fill) in [
            (&mut w.noninteractive, pal.panel),
            (&mut w.inactive, pal.card),
            (&mut w.hovered, pal.hover),
            (&mut w.active, pal.hover),
            (&mut w.open, pal.hover),
        ] {
            st.bg_fill = fill;
            st.weak_bg_fill = fill;
            st.corner_radius = CornerRadius::same(7);
            st.fg_stroke = Stroke::new(1.0, pal.text);
        }
        w.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
        w.inactive.bg_stroke = Stroke::new(1.0, pal.border);
        w.hovered.bg_stroke = Stroke::new(1.0, pal.accent.gamma_multiply(0.7));
        w.active.bg_stroke = Stroke::new(1.0, pal.accent);
        w.noninteractive.fg_stroke = Stroke::new(1.0, pal.dim);
    });
}

pub fn chip(ui: &mut egui::Ui, glyph: &str, text: &str, c: Color32) {
    egui::Frame::new()
        .fill(c.gamma_multiply(0.14))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal(|ui| {
                if !glyph.is_empty() {
                    ui.label(icon(glyph, 11.0, c));
                }
                ui.label(RichText::new(text).size(12.0).color(c));
            });
        });
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum RowAct {
    None,
    Toggle,
    Open,
    Delete,
    Pin,
}

/// Paint a round checkbox in `rect`. The row handles the click (see `item_row`).
fn paint_check(ui: &egui::Ui, rect: egui::Rect, done: bool, hovered: bool, ring: Color32, pal: &Pal) {
    let p = ui.painter();
    let c = rect.center();
    if done {
        p.circle_filled(c, 9.0, pal.accent);
        p.text(c, egui::Align2::CENTER_CENTER, ic::CHECK, icon_font(11.0), pal.on_accent);
    } else {
        p.circle_stroke(c, 8.5, Stroke::new(if hovered { 2.0 } else { 1.6 }, ring));
        if hovered {
            p.text(c, egui::Align2::CENTER_CENTER, ic::CHECK, icon_font(10.0), ring.gamma_multiply(0.8));
        }
    }
}

pub struct RowOpts {
    pub show_category: bool,
    pub selected: bool,
    pub compact: bool,
}

/// One item: checkbox, text, and a line of chips. Hover shows pin and delete.
pub fn item_row(ui: &mut egui::Ui, it: &Item, pal: &Pal, s: &Settings, o: &RowOpts) -> RowAct {
    let mut act = RowAct::None;
    let mut check: Option<egui::Rect> = None;
    let bg = ui.painter().add(egui::Shape::Noop);
    let pad = if o.compact { 4 } else { 7 };
    let now = chrono::Local::now();
    let inner = egui::Frame::new().inner_margin(Margin { left: 8, right: 8, top: pad, bottom: pad }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            if it.kind == Kind::Note {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, ic::NOTE, icon_font(14.0), pal.dim);
            } else {
                check = Some(rect); // painted below, once we know whether it's hovered
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                let mut t = RichText::new(&it.text).color(if it.done { pal.faint } else { pal.text });
                if it.done {
                    t = t.strikethrough();
                }
                let w = ui.available_width() - 56.0;
                ui.allocate_ui_with_layout(Vec2::new(w.max(60.0), 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                    ui.add(egui::Label::new(t).wrap().selectable(false));
                });
                // meta line
                let mut any = false;
                let meta = |ui: &mut egui::Ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    if it.pinned {
                        chip(ui, ic::PIN, "Pinned", pal.accent);
                    }
                    if let Some(d) = it.snoozed_until.filter(|_| !it.done) {
                        chip(ui, ic::SNOOZE, &format!("Snoozed to {}", store::describe_due(d, false, s.use_24h)), pal.amber);
                    } else if let Some(d) = it.due {
                        let overdue = it.is_overdue(now);
                        let today = d.date_naive() == now.date_naive();
                        let c = if it.done {
                            pal.faint
                        } else if overdue {
                            pal.red
                        } else if today {
                            pal.accent
                        } else {
                            pal.dim
                        };
                        let g = if it.remind { ic::BELL } else { ic::CALENDAR };
                        let mut txt = store::describe_due(d, it.all_day, s.use_24h);
                        if overdue {
                            txt = format!("{txt} · overdue");
                        }
                        chip(ui, g, &txt, c);
                    }
                    if let Some(r) = it.repeat {
                        chip(ui, ic::REPEAT, &r.label(), pal.dim);
                    }
                    if it.priority != Priority::None && !it.done {
                        chip(ui, ic::FLAG, it.priority.label(), priority_color(it.priority, pal));
                    }
                    if o.show_category && !it.category.is_empty() {
                        chip(ui, ic::LIST, &it.category, pal.dim);
                    }
                    for t in &it.tags {
                        chip(ui, "", &format!("#{t}"), pal.blue);
                    }
                    if it.done
                        && let Some(d) = it.done_at
                    {
                        chip(ui, ic::CHECK, &format!("Done {}", store::describe_due(d, false, s.use_24h)), pal.faint);
                    }
                };
                if it.pinned || it.due.is_some() || it.repeat.is_some() || it.priority != Priority::None || (o.show_category && !it.category.is_empty()) || !it.tags.is_empty() || it.done {
                    any = true;
                }
                if any {
                    ui.horizontal_wrapped(meta);
                }
            });
        });
    });
    let rect = inner.response.rect;
    let resp = ui.interact(rect, ui.scope_id().with(("row", it.id)), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    // The row takes the click; the left strip (circle plus a margin, full row height) ticks the
    // item and the rest opens it. One widget, so nothing on top can swallow the circle's clicks.
    let tick_zone = check.map(|c| egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top()), egui::pos2(c.right() + 6.0, rect.bottom())));
    let on_tick = tick_zone.is_some_and(|z| ui.rect_contains_pointer(z));
    if let Some(c) = check {
        paint_check(ui, c, it.done, on_tick, priority_ring(it.priority, pal), pal);
    }
    if on_tick {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let fill = if o.selected {
        pal.accent.gamma_multiply(0.13)
    } else if hovered {
        pal.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().set(bg, egui::Shape::rect_filled(rect, CornerRadius::same(8), fill));
    // priority stripe
    if it.priority >= Priority::Medium && !it.done {
        let r = egui::Rect::from_min_max(rect.left_top() + Vec2::new(0.0, 8.0), rect.left_bottom() + Vec2::new(3.0, -8.0));
        ui.painter().rect_filled(r, CornerRadius::same(2), priority_color(it.priority, pal));
    }
    if hovered {
        // hover actions at the right
        let size = 26.0;
        let mut x = rect.right() - 8.0 - size;
        let y = rect.top() + (if o.compact { 4.0 } else { 7.0 });
        for (g, a, tip) in [(ic::DELETE, RowAct::Delete, "Delete"), (ic::PIN, RowAct::Pin, if it.pinned { "Unpin" } else { "Pin to top" })] {
            let r = egui::Rect::from_min_size(egui::pos2(x, y), Vec2::splat(size));
            let b = ui.interact(r, ui.scope_id().with(("act", it.id, tip)), Sense::click()).on_hover_text(tip);
            let col = if b.hovered() { if a == RowAct::Delete { pal.red } else { pal.accent } } else { pal.dim };
            if b.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(6), pal.card);
            }
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, g, icon_font(13.0), col);
            if b.clicked() {
                act = a;
            }
            x -= size + 2.0;
        }
    }
    if act == RowAct::None && resp.clicked() {
        let at = resp.interact_pointer_pos();
        act = if tick_zone.zip(at).is_some_and(|(z, p)| z.contains(p)) { RowAct::Toggle } else { RowAct::Open };
    }
    act
}

fn priority_ring(p: Priority, pal: &Pal) -> Color32 {
    match p {
        Priority::None => pal.dim,
        _ => priority_color(p, pal),
    }
}

/// Open items in display order: pinned, overdue, priority, due, newest.
pub fn sort_open(items: &mut [&Item]) {
    let now = chrono::Local::now();
    items.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.is_overdue(now).cmp(&a.is_overdue(now)))
            .then(b.priority.cmp(&a.priority))
            .then(match (a.due, b.due) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
            .then(a.order.cmp(&b.order))
            .then(b.created.cmp(&a.created))
    });
}

/// Item from parsed text, with category defaults applied.
pub fn item_from(p: &crate::parse::Parsed, s: &Settings, fallback_cat: Option<&str>) -> Item {
    let now = chrono::Local::now();
    let due = p.due.and_then(|d| d.and_local_timezone(chrono::Local).single());
    let today_cat = s.categories.iter().find(|c| c.eq_ignore_ascii_case("today")).cloned();
    let category = p.category.clone().unwrap_or_else(|| {
        if s.smart_category
            && let (Some(d), Some(t)) = (due, &today_cat)
            && d.date_naive() == now.date_naive()
        {
            return t.clone();
        }
        fallback_cat.map(String::from).unwrap_or_else(|| s.default_category.clone())
    });
    Item {
        id: 0,
        text: p.text.clone(),
        kind: p.kind,
        done: false,
        created: now,
        done_at: None,
        due,
        all_day: p.all_day,
        remind: p.remind,
        fired: false,
        snoozed_until: None,
        repeat: p.repeat,
        priority: p.priority,
        category,
        tags: p.tags.clone(),
        pinned: false,
        order: 0,
    }
}

pub fn parse_with(text: &str, s: &Settings) -> crate::parse::Parsed {
    crate::parse::parse(
        text,
        chrono::Local::now().naive_local(),
        &crate::parse::Opts { categories: &s.categories, remind_when_timed: s.remind_when_timed, default_time: s.default_time() },
    )
}

/// Chips previewing what the typed text will become.
pub fn preview(ui: &mut egui::Ui, p: &crate::parse::Parsed, pal: &Pal, s: &Settings, cat: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (g, k) = match (p.kind, p.remind) {
            (Kind::Note, _) => (ic::NOTE, "Note"),
            (_, true) => (ic::BELL, "Reminder"),
            _ => (ic::CHECK, "Task"),
        };
        chip(ui, g, k, pal.accent);
        if let Some(d) = p.due.and_then(|d| d.and_local_timezone(chrono::Local).single()) {
            chip(ui, if p.remind { ic::CLOCK } else { ic::CALENDAR }, &store::describe_due(d, p.all_day, s.use_24h), pal.text);
        }
        if let Some(r) = p.repeat {
            chip(ui, ic::REPEAT, &r.label(), pal.dim);
        }
        if p.priority != Priority::None {
            chip(ui, ic::FLAG, p.priority.label(), priority_color(p.priority, pal));
        }
        chip(ui, ic::LIST, cat, pal.dim);
        for t in &p.tags {
            chip(ui, "", &format!("#{t}"), pal.blue);
        }
    });
}

// ---------------------------------------------------------------- watcher

#[derive(Default)]
pub struct Watch {
    pub items: AtomicBool,
    pub settings: AtomicBool,
    pub request: Mutex<Option<String>>,
}

/// Polls the data files four times a second and wakes the UI when one changes.
pub fn watch(ctx: egui::Context, want_requests: bool) -> Arc<Watch> {
    let w = Arc::new(Watch::default());
    let w2 = w.clone();
    std::thread::spawn(move || {
        let ip = store::items_path();
        let sp = store::settings_path();
        let rp = store::request_path();
        let mut im: Option<SystemTime> = store::mtime(&ip);
        let mut sm: Option<SystemTime> = store::mtime(&sp);
        if want_requests {
            let _ = std::fs::remove_file(&rp);
        }
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let mut wake = false;
            let m = store::mtime(&ip);
            if m != im {
                im = m;
                w2.items.store(true, Ordering::SeqCst);
                wake = true;
            }
            let m = store::mtime(&sp);
            if m != sm {
                sm = m;
                w2.settings.store(true, Ordering::SeqCst);
                wake = true;
            }
            if want_requests
                && let Ok(r) = std::fs::read_to_string(&rp)
            {
                let _ = std::fs::remove_file(&rp);
                *w2.request.lock().unwrap() = Some(r.trim().to_string());
                wake = true;
            }
            if wake {
                ctx.request_repaint();
            }
        }
    });
    w
}

/// Resize grips for borderless windows.
pub fn resize_edges(ui: &egui::Ui) {
    let ctx = ui.ctx();
    let rect = ctx.content_rect();
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let m = 6.0;
    let (l, r, t, b) = (p.x - rect.left() < m, rect.right() - p.x < m, p.y - rect.top() < m, rect.bottom() - p.y < m);
    use egui::ResizeDirection as D;
    let dir = match (l, r, t, b) {
        (true, _, true, _) => Some((D::NorthWest, egui::CursorIcon::ResizeNorthWest)),
        (_, true, true, _) => Some((D::NorthEast, egui::CursorIcon::ResizeNorthEast)),
        (true, _, _, true) => Some((D::SouthWest, egui::CursorIcon::ResizeSouthWest)),
        (_, true, _, true) => Some((D::SouthEast, egui::CursorIcon::ResizeSouthEast)),
        (true, ..) => Some((D::West, egui::CursorIcon::ResizeWest)),
        (_, true, ..) => Some((D::East, egui::CursorIcon::ResizeEast)),
        (_, _, true, _) => Some((D::North, egui::CursorIcon::ResizeNorth)),
        (.., true) => Some((D::South, egui::CursorIcon::ResizeSouth)),
        _ => None,
    };
    if let Some((d, c)) = dir {
        ctx.set_cursor_icon(c);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(d));
        }
    }
}

pub fn native_options(builder: egui::ViewportBuilder) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: builder.with_icon(crate::icon::egui_icon()),
        renderer: eframe::Renderer::Glow,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        centered: false,
        ..Default::default()
    }
}
