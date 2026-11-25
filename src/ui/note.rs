//! The sticky note: a small paper-coloured window with what's open and what got done today.
//! Started by the helper when "Show the sticky note on the desktop" is on.

use super::*;
use crate::settings::Settings;
use crate::store::{self, Data, Kind};
use crate::win;
use eframe::egui::{self, Align, Color32, CornerRadius, Layout, Margin, RichText, Vec2, ViewportCommand};
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub const COLORS: [(&str, Color32); 6] = [
    ("yellow", Color32::from_rgb(0xFF, 0xF1, 0x9C)),
    ("orange", Color32::from_rgb(0xFF, 0xD3, 0xA5)),
    ("pink", Color32::from_rgb(0xFF, 0xC7, 0xDC)),
    ("blue", Color32::from_rgb(0xC5, 0xE3, 0xFF)),
    ("green", Color32::from_rgb(0xCD, 0xF2, 0xC3)),
    ("charcoal", Color32::from_rgb(0x2A, 0x2D, 0x34)),
];

struct Note {
    settings: Settings,
    pal: Pal,
    paper: Color32,
    ink: Color32,
    data: Data,
    watch: Arc<Watch>,
    hwnd: Option<windows_sys::Win32::Foundation::HWND>,
    frames: u32,
    quick: String,
    last_pos: Option<(i32, i32)>,
}

fn paper_of(s: &Settings) -> (Color32, Color32) {
    let p = COLORS.iter().find(|c| c.0 == s.note_color).map(|c| c.1).unwrap_or(COLORS[0].1);
    let ink = if s.note_color == "charcoal" { Color32::from_rgb(0xEE, 0xEE, 0xEE) } else { Color32::from_rgb(0x33, 0x2B, 0x10) };
    (p, ink)
}

pub fn run() {
    if !win::claim(win::NOTE_MUTEX) {
        return;
    }
    let settings = Settings::load();
    let size = Vec2::from(settings.note_size);
    let mut vb = egui::ViewportBuilder::default()
        .with_title("Sticky note")
        .with_inner_size(size)
        .with_min_inner_size(Vec2::new(220.0, 160.0))
        .with_decorations(false)
        .with_resizable(true)
        .with_taskbar(false);
    if settings.note_on_top {
        vb = vb.with_always_on_top();
    }
    let _ = eframe::run_native(
        "Sticky note",
        native_options(vb),
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let pal = palette(&settings);
            apply_style(&cc.egui_ctx, &pal, &settings);
            let hwnd = win::hwnd_from(cc);
            if let Some(h) = hwnd {
                win::style_window(h, None);
                match settings.note_pos {
                    Some([x, y]) => win::move_window(h, x, y),
                    None => {
                        let a = win::primary_area();
                        win::move_window(h, a.right - (size.x * a.scale) as i32 - 24, a.top + 24);
                    }
                }
            }
            let (paper, ink) = paper_of(&settings);
            let watch = watch(cc.egui_ctx.clone(), false);
            Ok(Box::new(Note { settings, pal, paper, ink, data: store::load(), watch, hwnd, frames: 0, quick: String::new(), last_pos: None }))
        }),
    );
}

impl eframe::App for Note {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        self.paper.to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.watch.items.swap(false, Ordering::SeqCst) {
            self.data = store::load();
        }
        if self.watch.settings.swap(false, Ordering::SeqCst) {
            let s = Settings::load();
            if !s.note_window {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            if s.note_on_top != self.settings.note_on_top
                && let Some(h) = self.hwnd
            {
                win::set_topmost(h, s.note_on_top);
            }
            self.pal = palette(&s);
            (self.paper, self.ink) = paper_of(&s);
            self.settings = s;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        let (paper, ink) = (self.paper, self.ink);
        let dim = ink.gamma_multiply(0.6);
        // a pal tinted for the paper, so rows read well on it
        let mut pal = self.pal;
        pal.text = ink;
        pal.dim = dim;
        pal.faint = ink.gamma_multiply(0.45);
        pal.hover = ink.gamma_multiply(0.06);
        pal.card = paper;
        let accent = if self.settings.note_color == "charcoal" { pal.accent } else { Color32::from_rgb(0x8A, 0x6D, 0x00) };
        pal.accent = accent;
        pal.on_accent = paper;
        if self.settings.note_color != "charcoal" {
            // deeper colours that still read on pale paper
            pal.red = Color32::from_rgb(0xB0, 0x24, 0x1C);
            pal.amber = Color32::from_rgb(0x96, 0x55, 0x00);
            pal.blue = Color32::from_rgb(0x1F, 0x5F, 0xA8);
            pal.green = Color32::from_rgb(0x2E, 0x7D, 0x32);
        }

        let now = chrono::Local::now();
        let mut open: Vec<&store::Item> = self.data.items.iter().filter(|i| !i.done).collect();
        sort_open(&mut open);
        let open_ids: Vec<u64> = open.iter().map(|i| i.id).collect();
        let done_ids: Vec<u64> = self
            .data
            .items
            .iter()
            .filter(|i| i.done && i.done_at.is_some_and(|d| d.date_naive() == now.date_naive()))
            .map(|i| i.id)
            .collect();
        let tasks_open = open.iter().filter(|i| i.kind == Kind::Task).count();

        let mut toggle = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(paper).inner_margin(Margin { left: 14, right: 10, top: 10, bottom: 10 })).show(ui, |ui| {
            // header: drag handle first, so the buttons drawn on it get the clicks
            let top = ui.available_rect_before_wrap();
            let handle = egui::Rect::from_min_size(top.min, egui::vec2(top.width(), 24.0));
            if ui.interact(handle, egui::Id::unique("notedrag"), egui::Sense::drag()).drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sticky").font(semi(15.0)).color(ink));
                ui.label(RichText::new(format!("{tasks_open} open · {} done today", done_ids.len())).size(12.0).color(dim));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(icon(ic::CLOSE, 9.0, dim)).frame(false)).on_hover_text("Hide the note (turn it back on from the tray)").clicked() {
                        let mut s = Settings::load();
                        s.note_window = false;
                        s.save();
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    if ui.add(egui::Button::new(icon(ic::EXPAND, 11.0, dim)).frame(false)).on_hover_text("Open Sticky").clicked() {
                        crate::daemon::open_ui("full");
                    }
                });
            });
            ui.add_space(4.0);
            // quick add line
            let te = ui.add(
                egui::TextEdit::singleline(&mut self.quick)
                    .hint_text(RichText::new("+ add a task or reminder…").color(ink.gamma_multiply(0.55)))
                    .frame(egui::Frame::NONE)
                    .text_color(ink)
                    .desired_width(f32::INFINITY),
            );
            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.quick.trim().is_empty() {
                let p = parse_with(&self.quick, &self.settings);
                if !p.text.is_empty() {
                    let item = item_from(&p, &self.settings, None);
                    let t = item.text.clone();
                    let id = store::update(|d| d.add(item));
                    store::log("added", id, &t);
                    self.data = store::load();
                }
                self.quick.clear();
                te.request_focus();
            }
            let line = ui.available_rect_before_wrap();
            ui.painter().hline(line.x_range(), line.top(), egui::Stroke::new(1.0, ink.gamma_multiply(0.15)));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let opts = RowOpts { show_category: false, selected: false, compact: true };
                for id in &open_ids {
                    if let Some(it) = self.data.get(*id) {
                        match item_row(ui, it, &pal, &self.settings, &opts) {
                            RowAct::Toggle => toggle = Some((*id, true)),
                            RowAct::Delete => {
                                store::delete(*id);
                            }
                            RowAct::Pin => {
                                store::update(|d| {
                                    if let Some(i) = d.get_mut(*id) {
                                        i.pinned = !i.pinned;
                                    }
                                });
                            }
                            RowAct::Open => crate::daemon::open_ui("full"),
                            RowAct::None => {}
                        }
                    }
                }
                if open_ids.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("All clear.").color(dim));
                }
                if self.settings.note_show_done && !done_ids.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("DONE TODAY").size(10.5).color(dim).strong());
                    for id in &done_ids {
                        if let Some(it) = self.data.get(*id)
                            && item_row(ui, it, &pal, &self.settings, &opts) == RowAct::Toggle
                        {
                            toggle = Some((*id, false));
                        }
                    }
                }
            });
        });
        if let Some((id, d)) = toggle {
            store::set_done(id, d);
            self.data = store::load();
        }
        resize_edges(ui);
        // remember where it was put, once the mouse is let go
        if !ctx.input(|i| i.pointer.any_down())
            && let Some(h) = self.hwnd
        {
            let pos = win::window_pos(h);
            let size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
            if pos != self.last_pos && self.last_pos.is_some() {
                let mut s = Settings::load();
                if let Some((x, y)) = pos {
                    s.note_pos = Some([x, y]);
                }
                if let Some(sz) = size {
                    s.note_size = [sz.x, sz.y];
                }
                s.save();
            }
            self.last_pos = pos;
        }
        let _ = CornerRadius::ZERO;
        // refresh "overdue" colours and the frame counter now and then
        ctx.request_repaint_after(std::time::Duration::from_secs(if ctx.input(|i| i.pointer.has_pointer()) { 1 } else { 30 }));
    }
}
