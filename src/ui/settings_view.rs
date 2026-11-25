//! The searchable settings screen. Every change saves at once; other Sticky windows and the
//! helper pick it up within a second.

use super::main_window::MainApp;
use super::*;
use crate::settings::{META, SECTIONS, Settings};
use eframe::egui::{self, Align, CornerRadius, Layout, Margin, RichText, Stroke};

pub fn show(app: &mut MainApp, ui: &mut egui::Ui) {
    let pal = app.pal;
    egui::Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_height(40.0);
            ui.horizontal_centered(|ui| {
                ui.label(icon(ic::SEARCH, 14.0, pal.faint));
                let r = ui.add(
                    egui::TextEdit::singleline(&mut app.set_search)
                        .hint_text(RichText::new("Search settings: hotkey, theme, snooze, kite, dashboard…").color(pal.faint))
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY),
                );
                if app.set_search.is_empty() && !r.has_focus() && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_)))) {
                    r.request_focus();
                }
            });
        });
    ui.add_space(10.0);

    let q: Vec<String> = app.set_search.to_lowercase().split_whitespace().map(String::from).collect();
    let hit = |m: &crate::settings::Meta| {
        let hay = format!("{} {} {} {} {}", m.label, m.help, m.keywords, m.section, m.key).to_lowercase();
        q.iter().all(|w| hay.contains(w.as_str()))
    };
    let before = app.settings.clone();
    let mut s = app.settings.clone();
    let mut found = 0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for sec in SECTIONS {
            let rows: Vec<&crate::settings::Meta> = META.iter().filter(|m| m.section == sec && hit(m)).collect();
            if rows.is_empty() {
                continue;
            }
            found += rows.len();
            ui.add_space(6.0);
            ui.label(RichText::new(sec.to_uppercase()).size(11.0).color(pal.faint).strong());
            ui.add_space(2.0);
            egui::Frame::new().fill(pal.panel).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (n, m) in rows.iter().enumerate() {
                    if n > 0 {
                        let r = ui.available_rect_before_wrap();
                        ui.painter().hline(r.x_range().shrink(10.0), r.top(), Stroke::new(1.0, pal.border.gamma_multiply(0.6)));
                    }
                    row(ui, &pal, m, &mut s, &mut app.new_cat, &mut app.ha);
                }
            });
            ui.add_space(8.0);
        }
        if found == 0 {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("No settings match.").color(pal.faint)));
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Reset all to defaults").clicked() {
                let keep = (s.note_pos, s.main_size);
                s = Settings::default();
                s.note_pos = keep.0;
                s.main_size = keep.1;
            }
            ui.label(RichText::new(format!("Sticky {}", env!("CARGO_PKG_VERSION"))).color(pal.faint).size(12.0));
        });
    });
    if s != before {
        // a renamed category takes its items with it
        if s.categories.len() == before.categories.len() {
            let renames: Vec<(String, String)> =
                before.categories.iter().zip(&s.categories).filter(|(a, b)| a != b && !b.trim().is_empty() && !before.categories.contains(b) && !s.categories.contains(a)).map(|(a, b)| (a.clone(), b.clone())).collect();
            if !renames.is_empty() {
                crate::store::update(|d| {
                    for i in d.items.iter_mut() {
                        if let Some((_, new)) = renames.iter().find(|(old, _)| *old == i.category) {
                            i.category = new.clone();
                        }
                    }
                });
            }
        }
        s.save();
        app.settings = s;
        app.pal = palette(&app.settings);
        apply_style(ui.ctx(), &app.pal, &app.settings);
    }
}

fn row(ui: &mut egui::Ui, pal: &Pal, m: &crate::settings::Meta, s: &mut Settings, new_cat: &mut String, ha: &mut HaUi) {
    egui::Frame::new().inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let wide = matches!(m.key, "categories" | "ha");
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 300.0).max(220.0));
                ui.label(RichText::new(m.label).color(pal.text));
                ui.label(RichText::new(m.help).size(12.0).color(pal.dim));
            });
            if !wide {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if m.key.starts_with("ha_") {
                        ha_control(ui, pal, m.key, s, ha)
                    } else {
                        control(ui, pal, m.key, s)
                    }
                });
            }
        });
        if wide {
            ui.add_space(6.0);
            if m.key == "ha" {
                ha_connect(ui, pal, s, ha);
            } else {
                categories(ui, pal, s, new_cat);
            }
        }
    });
}

fn toggle(ui: &mut egui::Ui, pal: &Pal, on: &mut bool) {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 22.0), egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let fill = if *on { pal.accent } else { pal.card };
    ui.painter().rect(r, CornerRadius::same(11), fill, Stroke::new(1.0, if *on { pal.accent } else { pal.faint }), egui::StrokeKind::Inside);
    let x = egui::lerp((r.left() + 11.0)..=(r.right() - 11.0), t);
    ui.painter().circle_filled(egui::pos2(x, r.center().y), 7.0, if *on { pal.on_accent } else { pal.dim });
}

fn control(ui: &mut egui::Ui, pal: &Pal, key: &str, s: &mut Settings) {
    match key {
        "hotkey" => {
            let ok = global_hotkey::hotkey::HotKey::from_str(&s.hotkey.replace(' ', "")).is_ok();
            ui.add(egui::TextEdit::singleline(&mut s.hotkey).desired_width(150.0));
            ui.label(if ok { icon(ic::CHECK, 12.0, pal.green) } else { icon(ic::CLOSE, 11.0, pal.red) }).on_hover_text(if ok {
                "Valid. Takes effect within a second."
            } else {
                "Not a valid key combination"
            });
        }
        "kite_keyword" => {
            if ui.button("Add to Kite").on_hover_text("Adds a runner command to Kite's settings (a backup is kept)").clicked() {
                let msg = match crate::kite::install(&s.kite_keyword) {
                    Ok(m) => m,
                    Err(e) => format!("Couldn't update Kite: {e}"),
                };
                crate::store::log("kite", 0, &msg);
            }
            ui.add(egui::TextEdit::singleline(&mut s.kite_keyword).desired_width(60.0));
        }
        "start_with_windows" => toggle(ui, pal, &mut s.start_with_windows),
        "close_capture_after_add" => toggle(ui, pal, &mut s.close_capture_after_add),
        "capture_shows_list" => toggle(ui, pal, &mut s.capture_shows_list),
        "compact" => toggle(ui, pal, &mut s.compact),
        "use_24h" => toggle(ui, pal, &mut s.use_24h),
        "smart_category" => toggle(ui, pal, &mut s.smart_category),
        "confirm_delete" => toggle(ui, pal, &mut s.confirm_delete),
        "remind_when_timed" => toggle(ui, pal, &mut s.remind_when_timed),
        "sound" => {
            toggle(ui, pal, &mut s.sound);
            if ui.button(format!("{}  Test", ic::BELL)).clicked() {
                { s.save(); crate::sound::ring(&s.sound_name); }
            }
            let sounds = crate::sound::SOUNDS;
            egui::ComboBox::from_id_salt("sound_name")
                .selected_text(sounds.iter().find(|x| x.0 == s.sound_name).map(|x| x.1).unwrap_or("Alarm"))
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (k, l, _) in sounds {
                        ui.selectable_value(&mut s.sound_name, k.to_string(), l);
                    }
                });
        }
        "sound_repeat" => toggle(ui, pal, &mut s.sound_repeat),
        "sound_duck" => toggle(ui, pal, &mut s.sound_duck),
        "sound_boost" => {
            ui.add(egui::Slider::new(&mut s.sound_boost, 1..=8).custom_formatter(|v, _| format!("x{v}")));
        }
        "window_screen" => screen_picker(ui, "window_screen", &mut s.window_screen),
        "note_window" => toggle(ui, pal, &mut s.note_window),
        "note_on_top" => toggle(ui, pal, &mut s.note_on_top),
        "note_show_done" => toggle(ui, pal, &mut s.note_show_done),
        "auto_update" => toggle(ui, pal, &mut s.auto_update),
        "check_updates" => {
            toggle(ui, pal, &mut s.check_updates);
            if ui.button("Check now").clicked() {
                crate::win::spawn_self(&["update"]);
            }
            ui.label(RichText::new(format!("v{}", crate::update::VERSION)).size(12.0).color(pal.faint));
        }
        "theme" => {
            egui::ComboBox::from_id_salt("theme")
                .selected_text(THEMES.iter().find(|t| t.0 == s.theme).map(|t| t.1).unwrap_or("Midnight"))
                .width(160.0)
                .show_ui(ui, |ui| {
                    for (k, l) in THEMES {
                        ui.selectable_value(&mut s.theme, k.to_string(), l);
                    }
                });
        }
        "accent" => {
            let presets = ["#FFC83D", "#FF8A3D", "#FF5C8A", "#C2185B", "#9B7BFF", "#4C9AFF", "#2EC4B6", "#5BD08A"];
            let mut c = hex(&s.accent).unwrap_or(pal.accent);
            if ui.color_edit_button_srgba(&mut c).changed() {
                s.accent = format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
            }
            for p in presets.iter().rev() {
                let col = hex(p).unwrap();
                let (r, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().circle_filled(r.center(), 8.0, col);
                if s.accent.eq_ignore_ascii_case(p) {
                    ui.painter().circle_stroke(r.center(), 10.0, Stroke::new(1.5, pal.text));
                }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    s.accent = p.to_string();
                }
            }
        }
        "font_scale" => {
            ui.add(egui::Slider::new(&mut s.font_scale, 0.8..=1.4).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
        }
        "default_category" => {
            egui::ComboBox::from_id_salt("defcat").selected_text(&s.default_category).width(160.0).show_ui(ui, |ui| {
                for c in s.categories.clone() {
                    ui.selectable_value(&mut s.default_category, c.clone(), c);
                }
            });
        }
        "show_done_days" => {
            ui.add(egui::DragValue::new(&mut s.show_done_days).range(0..=365).suffix(" days"));
        }
        "default_time" => {
            let ok = chrono::NaiveTime::parse_from_str(&s.default_time, "%H:%M").is_ok();
            ui.add(egui::TextEdit::singleline(&mut s.default_time).desired_width(70.0));
            if !ok {
                ui.label(RichText::new("HH:MM").color(pal.red).size(12.0));
            }
        }
        "snooze_minutes" => {
            ui.add(egui::DragValue::new(&mut s.snooze_minutes).range(1..=720).suffix(" min"));
        }
        "popup_corner" => {
            for (k, l) in [("center", "Centre"), ("top-right", "Top right"), ("bottom-right", "Bottom right")] {
                ui.selectable_value(&mut s.popup_corner, k.to_string(), l);
            }
        }
        "popup_monitor" => screen_picker(ui, "popup_monitor", &mut s.popup_monitor),
        "note_color" => {
            for (k, c) in super::note::COLORS.iter().rev() {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                ui.painter().rect_filled(r.shrink(2.0), CornerRadius::same(4), *c);
                if s.note_color == *k {
                    ui.painter().rect_stroke(r, CornerRadius::same(5), Stroke::new(1.5, pal.text), egui::StrokeKind::Inside);
                }
                if resp.on_hover_text(*k).clicked() {
                    s.note_color = k.to_string();
                }
            }
        }
        "data" => {
            if ui.button(format!("{}  Open folder", ic::FOLDER)).clicked() {
                crate::win::open_path(&crate::store::data_dir());
            }
            ui.label(RichText::new(crate::store::data_dir().display().to_string()).size(12.0).color(pal.faint));
        }
        _ => {}
    }
}

/// Main display, where the mouse is, or one particular connected screen.
fn screen_picker(ui: &mut egui::Ui, id: &str, value: &mut String) {
    let screens = crate::win::screens();
    let label = match value.as_str() {
        "primary" | "" => "Main display".to_string(),
        "cursor" => "Where the mouse is".to_string(),
        v => screens.iter().find(|x| x.id == v).map(|x| x.label()).unwrap_or_else(|| "Main display (that screen isn't connected)".into()),
    };
    egui::ComboBox::from_id_salt(id).selected_text(label).width(230.0).show_ui(ui, |ui| {
        ui.selectable_value(value, "primary".to_string(), "Main display");
        ui.selectable_value(value, "cursor".to_string(), "Where the mouse is");
        if screens.len() > 1 {
            ui.separator();
            for sc in &screens {
                ui.selectable_value(value, sc.id.clone(), sc.label());
            }
        }
    });
}

fn categories(ui: &mut egui::Ui, pal: &Pal, s: &mut Settings, new_cat: &mut String) {
    let mut remove = None;
    let mut up = None;
    let n = s.categories.len();
    for i in 0..n {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.add(egui::TextEdit::singleline(&mut s.categories[i]).desired_width(200.0));
            if ui.add_enabled(i > 0, egui::Button::new(icon("\u{E70E}", 11.0, pal.dim)).frame(false)).on_hover_text("Move up").clicked() {
                up = Some(i);
            }
            if ui.add_enabled(n > 1, egui::Button::new(icon(ic::DELETE, 11.0, pal.dim)).frame(false)).on_hover_text("Remove (its items show under Other)").clicked() {
                remove = Some(i);
            }
            let tag: String = s.categories[i].chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
            ui.label(RichText::new(format!("#{tag}")).size(12.0).color(pal.faint));
        });
    }
    if let Some(i) = up {
        s.categories.swap(i, i - 1);
    }
    if let Some(i) = remove {
        s.categories.remove(i);
    }
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let r = ui.add(egui::TextEdit::singleline(new_cat).hint_text("New folder").desired_width(200.0));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Add").clicked() || enter) && !new_cat.trim().is_empty() {
            s.categories.push(new_cat.trim().to_string());
            new_cat.clear();
        }
    });
    if !s.categories.contains(&s.default_category)
        && let Some(f) = s.categories.first()
    {
        s.default_category = f.clone();
    }
}

use std::str::FromStr;

// ---------------------------------------------------------------- Home Assistant

/// Settings-page state for the Home Assistant rows.
#[derive(Default)]
pub struct HaUi {
    init: bool,
    url: String,
    token: String,
    /// What Home Assistant has (printers, cameras), fetched in the background.
    found: std::sync::Arc<std::sync::Mutex<Option<Result<crate::ha::Found, String>>>>,
    looked: String,
    status: crate::ha::Status,
    status_at: Option<std::time::Instant>,
    msg: String,
}

impl HaUi {
    /// Look up printers and cameras once per address and token.
    fn discover(&mut self, s: &Settings, ctx: &egui::Context) {
        let key = format!("{}|{}", s.ha_url, s.ha_token);
        if key == self.looked {
            return;
        }
        self.looked = key;
        let Some(conn) = crate::ha::Conn::from_settings(s) else {
            *self.found.lock().unwrap() = None;
            return;
        };
        let slot = self.found.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = crate::ha::discover(&conn);
            *slot.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
    }
    fn found(&self) -> Option<crate::ha::Found> {
        self.found.lock().ok()?.as_ref()?.as_ref().ok().cloned()
    }
}

fn ha_connect(ui: &mut egui::Ui, pal: &Pal, s: &mut Settings, ha: &mut HaUi) {
    if !ha.init {
        ha.init = true;
        ha.url = s.ha_url.clone();
    }
    if ha.status_at.is_none_or(|t| t.elapsed().as_secs() >= 1) {
        ha.status = crate::ha::load_status();
        ha.status_at = Some(std::time::Instant::now());
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    ha.discover(s, ui.ctx());
    let saved = crate::secret::open(&s.ha_token).is_some();
    egui::Grid::new("ha_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(RichText::new("Address").color(pal.dim));
        ui.add(egui::TextEdit::singleline(&mut ha.url).hint_text("https://homeassistant.example.com").desired_width(320.0));
        ui.end_row();
        ui.label(RichText::new("Token").color(pal.dim));
        let hint = if saved { "Saved. Paste a new one to replace it." } else { "Paste a long-lived access token" };
        ui.add(egui::TextEdit::singleline(&mut ha.token).password(true).hint_text(hint).desired_width(320.0));
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let ok = crate::net::Url::parse(&ha.url).is_some() && (saved || !ha.token.trim().is_empty());
        let label = if saved && ha.token.is_empty() && ha.url.trim().trim_end_matches('/') == s.ha_url { "Reconnect" } else { "Connect" };
        if ui.add_enabled(ok, egui::Button::new(label)).clicked() {
            s.ha_url = ha.url.trim().trim_end_matches('/').to_string();
            if !ha.token.trim().is_empty() {
                s.ha_token = crate::secret::seal(&ha.token);
                ha.token.clear();
            } else {
                // same settings: touch the file so the helper reconnects
                s.ha_token = crate::secret::seal(&crate::secret::open(&s.ha_token).unwrap_or_default());
            }
            ha.looked.clear();
            ha.msg.clear();
        }
        if !saved
            && let Some(p) = crate::secret::desk_dashboard_credential()
            && ui.button("Use the desk dashboard's sign-in").on_hover_text(p.display().to_string()).clicked()
        {
            match crate::secret::read_clixml(&p) {
                Some((url, token)) if crate::net::Url::parse(&url).is_some() && !token.is_empty() => {
                    s.ha_url = url.trim_end_matches('/').to_string();
                    s.ha_token = crate::secret::seal(&token);
                    ha.url = s.ha_url.clone();
                    ha.looked.clear();
                    ha.msg.clear();
                }
                _ => ha.msg = "Couldn't read the desk dashboard's sign-in.".into(),
            }
        }
        if saved && ui.button("Disconnect").on_hover_text("Forgets the token").clicked() {
            s.ha_token.clear();
            ha.looked.clear();
        }
    });
    ui.add_space(4.0);
    let st = &ha.status;
    let (col, word) = match st.state.as_str() {
        "connected" => (pal.green, "Connected"),
        "connecting" => (pal.accent, "Connecting"),
        "error" => (pal.red, "Can't connect"),
        _ => (pal.faint, "Off"),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("●").color(col));
        let mut line = format!("{word}: {}", st.detail);
        if st.state == "connected" && !st.printer.is_empty() && s.ha_print_alerts {
            line += &format!(" · watching the {} printer", st.printer);
            if !st.camera.is_empty() {
                let cam = ha.found().and_then(|f| f.cameras.iter().find(|c| c.0 == st.camera).map(|c| c.1.clone())).unwrap_or_else(|| st.camera.clone());
                line += &format!(", photo from {cam}");
            }
        }
        ui.label(RichText::new(line).size(12.0).color(pal.dim));
    });
    if let Some(Err(e)) = ha.found.lock().ok().and_then(|f| f.clone()) {
        ui.label(RichText::new(e).size(12.0).color(pal.red));
    }
    if !ha.msg.is_empty() {
        ui.add(egui::Label::new(RichText::new(&ha.msg).size(12.0).color(pal.dim)).wrap());
    }
}

fn ha_control(ui: &mut egui::Ui, pal: &Pal, key: &str, s: &mut Settings, ha: &mut HaUi) {
    let found = ha.found();
    match key {
        "ha_print_alerts" => toggle(ui, pal, &mut s.ha_print_alerts),
        "ha_notifications" => {
            toggle(ui, pal, &mut s.ha_notifications);
            let b = ui.add_enabled(!s.ha_url.is_empty(), egui::Button::new("Copy webhook setup"));
            if b.on_hover_text("Copies a Home Assistant automation to paste in (Automations > Create > Edit in YAML)").clicked() {
                let id = webhook_id();
                ui.ctx().copy_text(webhook_yaml(&id));
                ha.msg = format!(
                    "Copied. In Home Assistant: Settings > Automations > Create automation > ⋮ > Edit in YAML, paste over everything, save.\nThen send a POST with JSON to {}/api/webhook/{id}\n    {{\"title\": \"Print done\", \"message\": \"Took 2h\", \"camera\": \"camera.printer\"}}   (\"image\": \"https://…\" works too)\nAnyone who has that address can send you alerts, so keep it private.",
                    s.ha_url.trim_end_matches('/')
                );
            }
        }
        "ha_printer" => {
            let label = if s.ha_printer.is_empty() {
                "First one found".to_string()
            } else {
                found.as_ref().and_then(|f| f.printers.iter().find(|p| p.0 == s.ha_printer).map(|p| p.1.clone())).unwrap_or_else(|| s.ha_printer.clone())
            };
            egui::ComboBox::from_id_salt("ha_printer").selected_text(label).width(230.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.ha_printer, String::new(), "First one found");
                for (id, name) in found.iter().flat_map(|f| f.printers.iter()) {
                    ui.selectable_value(&mut s.ha_printer, id.clone(), format!("{name}  ({id})"));
                }
            });
        }
        "ha_camera" => {
            let label = match s.ha_camera.as_str() {
                "" => "The printer's own camera".to_string(),
                "none" => "No photo".to_string(),
                c => found.as_ref().and_then(|f| f.cameras.iter().find(|x| x.0 == c).map(|x| x.1.clone())).unwrap_or_else(|| c.to_string()),
            };
            egui::ComboBox::from_id_salt("ha_camera").selected_text(label).width(230.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.ha_camera, String::new(), "The printer's own camera");
                ui.selectable_value(&mut s.ha_camera, "none".to_string(), "No photo");
                for (id, name) in found.iter().flat_map(|f| f.cameras.iter()) {
                    ui.selectable_value(&mut s.ha_camera, id.clone(), name.as_str());
                }
            });
        }
        _ => {}
    }
}

/// A hard-to-guess webhook id: anyone who knows it can send alerts.
fn webhook_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::from("sticky-");
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out += &format!("{:016x}", h.finish());
    }
    out
}

fn webhook_yaml(id: &str) -> String {
    let t = r#"alias: Sticky alert (webhook)
description: >-
  POST JSON to /api/webhook/WEBHOOK_ID to pop up an alert in Sticky:
  {"title": "...", "message": "...", "camera": "camera.x"} or "image": "https://..."
triggers:
  - trigger: webhook
    webhook_id: WEBHOOK_ID
    allowed_methods: [POST, PUT]
    local_only: false
actions:
  - action: persistent_notification.create
    data:
      notification_id: "sticky_{{ (now().timestamp() * 1000) | int }}"
      title: "{{ trigger.json.title | default('Alert') }}"
      message: >-
        {{ trigger.json.message | default('') }}
        {% if trigger.json.camera is defined %}![photo](/api/camera_proxy/{{ trigger.json.camera }}){% elif trigger.json.image is defined %}![photo]({{ trigger.json.image }}){% endif %}
mode: queued
"#;
    t.replace("WEBHOOK_ID", id)
}
