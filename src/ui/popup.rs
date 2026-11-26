//! The reminder pop-up: stays on top until you tick it off, snooze it or dismiss it.

use super::*;
use crate::settings::Settings;
use crate::store;
use crate::win;
use chrono::{Local, TimeZone};
use eframe::egui::{self, Align, CornerRadius, Layout, Margin, RichText, Vec2, ViewportCommand};

struct Popup {
    id: u64,
    text: String,
    was: chrono::DateTime<Local>,
    settings: Settings,
    pal: Pal,
    hwnd: Option<windows_sys::Win32::Foundation::HWND>,
    frames: u32,
    snooze_open: bool,
    last_h: f32,
    slot: i32,
    repeats: bool,
    rang: u32,
    last_ring: std::time::Instant,
    detail: String,
    notes: String,
    steps: Vec<crate::store::Step>,
    source: String,
    picture: Option<(egui::TextureHandle, std::path::PathBuf)>,
    /// The item's own sound (doorbell), else the settings' one
    sound: String,
    /// A camera shown live (doorbell): frames arrive on `live_rx` until `live_until`
    live: String,
    live_rx: Option<std::sync::mpsc::Receiver<(u32, u32, Vec<u8>)>>,
    live_until: Option<std::time::Instant>,
    live_auto: bool,
}

const W: f32 = 400.0;
/// How long the doorbell picture stays live after it pops up, or after a click
const LIVE_FOR: std::time::Duration = std::time::Duration::from_secs(180);

pub fn run(id: u64, slot: i32, was: i64) {
    let data = store::load();
    let Some(item) = data.get(id).cloned() else { return };
    let settings = Settings::load();
    let was = (was > 0).then(|| Local.timestamp_opt(was, 0).single()).flatten().or(item.due).unwrap_or_else(Local::now);
    let vb = egui::ViewportBuilder::default()
        .with_title(format!("Sticky reminder {id}"))
        .with_inner_size(Vec2::new(W, 150.0))
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(true)
        .with_active(std::env::var_os("STICKY_NO_FOCUS").is_none());
    let _ = eframe::run_native(
        "Sticky reminder",
        native_options(vb),
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let pal = palette(&settings);
            apply_style(&cc.egui_ctx, &pal, &settings);
            let hwnd = win::hwnd_from(cc);
            if let Some(h) = hwnd {
                win::style_window(h, Some(u32::from_le_bytes([pal.accent.r(), pal.accent.g(), pal.accent.b(), 0])));
                place(h, &settings, slot, 150.0);
            }
            // the photo on an alert, decoded at twice the pop-up width for sharp high-DPI screens
            let picture = item.picture.as_ref().and_then(|name| {
                let path = crate::picture::path(name);
                let bytes = std::fs::read(&path).ok()?;
                let (w, h, px) = crate::picture::decode(&bytes, (W * 2.0) as u32)?;
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                Some((cc.egui_ctx.load_texture("picture", img, egui::TextureOptions::LINEAR), path))
            });
            Ok(Box::new(Popup {
                id,
                text: item.text.clone(),
                was,
                settings,
                pal,
                hwnd,
                frames: 0,
                snooze_open: false,
                last_h: 150.0,
                slot,
                repeats: item.repeat.is_some(),
                rang: 0,
                last_ring: std::time::Instant::now(),
                detail: item.detail.clone(),
                notes: item.notes.clone(),
                steps: item.steps.clone(),
                source: item.source.clone(),
                picture,
                sound: item.sound.clone(),
                live: item.live.clone(),
                live_rx: None,
                live_until: None,
                // live straight away only for a fresh alert, not one snoozed or shown late
                live_auto: !item.live.is_empty() && (Local::now() - item.created).num_minutes() < 10,
            }))
        }),
    );
}

fn place(h: windows_sys::Win32::Foundation::HWND, s: &Settings, slot: i32, height: f32) {
    let a = win::screen_area(&s.popup_monitor);
    let w = (W * a.scale) as i32;
    let hh = (height * a.scale) as i32;
    let gap = (14.0 * a.scale) as i32;
    let step = hh + gap;
    let (x, y) = match s.popup_corner.as_str() {
        "center" => (a.left + (a.right - a.left - w) / 2, a.top + (a.bottom - a.top - hh) / 3 + slot * step),
        "top-right" => (a.right - w - gap, a.top + gap + slot * step),
        _ => (a.right - w - gap, a.bottom - hh - gap - slot * step),
    };
    win::move_window(h, x, y);
}

impl Popup {
    /// Fetch the camera's picture over and over (about three a second) for `LIVE_FOR`.
    fn start_live(&mut self, ctx: &egui::Context) {
        let Some(conn) = crate::ha::Conn::from_settings(&self.settings) else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let cam = self.live.clone();
        let ctx = ctx.clone();
        let until = std::time::Instant::now() + LIVE_FOR;
        std::thread::spawn(move || {
            while std::time::Instant::now() < until {
                let t0 = std::time::Instant::now();
                match crate::net::get(&conn.url, &format!("/api/camera_proxy/{cam}"), Some(&conn.token)) {
                    Ok(b) => {
                        if let Some(f) = crate::picture::decode(&b, (W * 2.0) as u32) {
                            if tx.send(f).is_err() {
                                break;
                            }
                            ctx.request_repaint();
                        }
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_secs(1)),
                }
                let spent = t0.elapsed();
                let gap = std::time::Duration::from_millis(300);
                if spent < gap {
                    std::thread::sleep(gap - spent);
                }
            }
            ctx.request_repaint();
        });
        self.live_rx = Some(rx);
        self.live_until = Some(until);
    }

    fn is_live(&self) -> bool {
        self.live_until.is_some_and(|t| std::time::Instant::now() < t)
    }
}

impl eframe::App for Popup {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        self.pal.panel.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Drag to move. Added first so the buttons drawn later get the clicks.
        let bg = ui.interact(ctx.content_rect(), egui::Id::unique("drag"), egui::Sense::drag());
        if bg.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        self.frames += 1;
        if self.frames == 2
            && let Some(h) = self.hwnd
        {
            win::set_topmost(h, true);
        }
        // Sound once the pop-up is on screen, then every minute if asked (up to 10 times).
        if self.frames == 1 && self.live_auto {
            self.start_live(&ctx);
        }
        if let Some(rx) = &self.live_rx
            && let Some((w, h, px)) = rx.try_iter().last()
        {
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            match &mut self.picture {
                Some((tex, _)) => tex.set(img, egui::TextureOptions::LINEAR),
                None => self.picture = Some((ctx.load_texture("picture", img, egui::TextureOptions::LINEAR), std::path::PathBuf::new())),
            }
        }
        if !self.sound.is_empty() {
            if self.rang == 0 && self.frames >= 2 {
                crate::sound::ring(&self.sound);
                self.rang = 1;
            } else if self.rang == 0 {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        } else if self.settings.sound {
            let due = if self.rang == 0 { self.frames >= 2 } else { self.settings.sound_repeat && self.rang < 10 && self.last_ring.elapsed().as_secs() >= 60 };
            if due {
                crate::sound::ring(&self.settings.sound_name);
                self.rang += 1;
                self.last_ring = std::time::Instant::now();
            }
            if self.rang == 0 || (self.settings.sound_repeat && self.rang < 10) {
                ctx.request_repaint_after(std::time::Duration::from_millis(if self.rang == 0 { 50 } else { 1000 }));
            }
        }
        let pal = self.pal;
        let mut close = false;
        let r = egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.panel).inner_margin(Margin { left: 18, right: 14, top: 14, bottom: 14 })).show(ui, |ui| {
            let top = ui.cursor().top();
            // accent bar
            let full = ui.ctx().content_rect();
            ui.painter().rect_filled(egui::Rect::from_min_size(full.left_top(), Vec2::new(4.0, full.height())), CornerRadius::ZERO, pal.accent);
            ui.horizontal(|ui| {
                ui.label(icon(ic::BELL, 15.0, pal.accent));
                let head = if self.source.is_empty() { "REMINDER".to_string() } else { self.source.to_uppercase() };
                ui.label(RichText::new(head).size(11.0).color(pal.accent).strong());
                let ago = (Local::now() - self.was).num_minutes();
                let when = store::describe_due(self.was, false, self.settings.use_24h);
                let txt = if ago >= 2 { format!("{when} · {ago} min ago") } else { when };
                ui.label(RichText::new(txt).size(12.0).color(pal.faint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(icon(ic::CLOSE, 10.0, pal.dim)).frame(false)).on_hover_text("Dismiss").clicked() {
                        store::dismiss(self.id);
                        close = true;
                    }
                });
            });
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(&self.text).font(semi(18.0 * self.settings.font_scale)).color(pal.text)).wrap());
            if !self.detail.is_empty() {
                ui.add_space(2.0);
                ui.add(egui::Label::new(RichText::new(&self.detail).size(13.0 * self.settings.font_scale).color(pal.dim)).wrap());
            }
            if !self.steps.is_empty() {
                ui.add_space(4.0);
                let mut changed = false;
                for st in self.steps.iter_mut() {
                    let t = if st.done { RichText::new(&st.text).color(pal.faint).strikethrough() } else { RichText::new(&st.text).color(pal.text) };
                    if ui.checkbox(&mut st.done, t).changed() {
                        changed = true;
                    }
                }
                if changed && crate::store::set_steps(self.id, self.steps.clone(), !self.repeats) {
                    close = true; // all steps done: the item is done
                }
            }
            if !self.notes.trim().is_empty() {
                ui.add_space(4.0);
                let n: String = self.notes.trim().chars().take(600).collect();
                ui.add(egui::Label::new(RichText::new(n).size(13.0 * self.settings.font_scale).color(pal.dim)).wrap());
            }
            let links = crate::parse::links(&format!("{} {} {}", self.text, self.detail, self.notes));
            if !links.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(icon(ic::LINK, 12.0, pal.dim));
                    for l in &links {
                        ui.hyperlink_to(RichText::new(crate::parse::link_label(l)).color(pal.blue), l).on_hover_text(l.as_str());
                    }
                });
            }
            let mut watch_again = false;
            if let Some((tex, path)) = &self.picture {
                ui.add_space(8.0);
                let w = ui.available_width();
                let sz = tex.size_vec2();
                let h = (w * sz.y / sz.x).min(420.0);
                let r = ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::new(h * sz.x / sz.y, h)).corner_radius(8).sense(egui::Sense::click()));
                if !self.live.is_empty() {
                    let live = self.is_live();
                    let (txt, col) = if live { ("●  LIVE", pal.red) } else { ("Click to watch live", pal.text) };
                    let font = egui::FontId::proportional(11.5);
                    let galley = ui.painter().layout_no_wrap(txt.to_string(), font, col);
                    let at = r.rect.left_top() + Vec2::new(8.0, 8.0);
                    let bg = egui::Rect::from_min_size(at, galley.size() + Vec2::new(12.0, 6.0));
                    ui.painter().rect_filled(bg, CornerRadius::same(5), egui::Color32::from_black_alpha(170));
                    ui.painter().galley(at + Vec2::new(6.0, 3.0), galley, col);
                    let tip = if live { "Live from the doorbell camera" } else { "Watch the doorbell camera live again" };
                    if r.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !live {
                        watch_again = true;
                    }
                    if live {
                        // the badge turns back when the time is up
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                    }
                } else if r.on_hover_text("Open the picture").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    let _ = std::process::Command::new("explorer").arg(path).spawn();
                }
            }
            if watch_again {
                self.start_live(ui.ctx());
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let done = egui::Button::new(RichText::new(format!("{}  Done", ic::CHECK)).color(pal.on_accent)).fill(pal.accent).min_size(Vec2::new(96.0, 30.0));
                if ui.add(done).clicked() {
                    if self.repeats {
                        // the helper already moved it to the next occurrence
                        store::log("done (repeats)", self.id, &self.text);
                    } else {
                        store::set_done(self.id, true);
                    }
                    close = true;
                }
                let sn = self.settings.snooze_minutes;
                if ui.add(egui::Button::new(format!("{}  Snooze {sn} min", ic::SNOOZE)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    store::snooze(self.id, sn as i64);
                    close = true;
                }
                if ui.add(egui::Button::new("…").min_size(Vec2::new(30.0, 30.0))).on_hover_text("More snooze options").clicked() {
                    self.snooze_open = !self.snooze_open;
                }
            });
            if self.snooze_open {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (l, m) in [("5 min", 5), ("30 min", 30), ("1 hour", 60), ("3 hours", 180)] {
                        if ui.button(l).clicked() {
                            store::snooze(self.id, m);
                            close = true;
                        }
                    }
                    if ui.button("Tomorrow 9am").clicked() {
                        let t = (Local::now() + chrono::Duration::days(1)).date_naive().and_time(self.settings.default_time());
                        let mins = (t.and_local_timezone(Local).single().unwrap_or_else(Local::now) - Local::now()).num_minutes();
                        store::snooze(self.id, mins.max(1));
                        close = true;
                    }
                });
            }
            ui.cursor().top() - top
        });
        let h = (r.inner.ceil() + 28.0).clamp(100.0, 800.0);
        if (h - self.last_h).abs() > 1.0 {
            self.last_h = h;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(W, h)));
            if let Some(hw) = self.hwnd {
                place(hw, &self.settings, self.slot, h);
            }
        }
        if close {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        // keep the "min ago" fresh
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }
}
