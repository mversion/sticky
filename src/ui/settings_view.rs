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
                    row(ui, &pal, m, &mut s, &mut app.new_cat);
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

fn row(ui: &mut egui::Ui, pal: &Pal, m: &crate::settings::Meta, s: &mut Settings, new_cat: &mut String) {
    egui::Frame::new().inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let wide = matches!(m.key, "categories");
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 300.0).max(220.0));
                ui.label(RichText::new(m.label).color(pal.text));
                ui.label(RichText::new(m.help).size(12.0).color(pal.dim));
            });
            if !wide {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| control(ui, pal, m.key, s));
            }
        });
        if wide {
            ui.add_space(6.0);
            categories(ui, pal, s, new_cat);
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
        "sound" => toggle(ui, pal, &mut s.sound),
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
        "popup_monitor" => {
            for (k, l) in [("cursor", "Where the mouse is"), ("primary", "Main screen")] {
                ui.selectable_value(&mut s.popup_monitor, k.to_string(), l);
            }
        }
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
            if ui.add_enabled(n > 1, egui::Button::new(icon(ic::DELETE, 11.0, pal.dim)).frame(false)).on_hover_text("Remove (items keep their category under Other)").clicked() {
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
        let r = ui.add(egui::TextEdit::singleline(new_cat).hint_text("New category").desired_width(200.0));
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
