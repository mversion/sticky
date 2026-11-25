//! A one-and-a-half-second "Added" confirmation after adding from Kite or the command line.

use super::*;
use crate::settings::Settings;
use crate::win;
use eframe::egui::{self, Margin, RichText, Vec2, ViewportCommand};
use std::time::Instant;

struct Toast {
    title: String,
    detail: String,
    pal: Pal,
    start: Instant,
}

pub fn run(title: String, detail: String) {
    let settings = Settings::load();
    let vb = egui::ViewportBuilder::default()
        .with_title("Sticky")
        .with_inner_size(Vec2::new(420.0, 64.0))
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_active(false);
    let _ = eframe::run_native(
        "Sticky",
        native_options(vb),
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let pal = palette(&settings);
            apply_style(&cc.egui_ctx, &pal, &settings);
            if let Some(h) = win::hwnd_from(cc) {
                win::style_window(h, None);
                let a = win::screen_area(&settings.window_screen);
                let w = (420.0 * a.scale) as i32;
                win::move_window(h, a.left + (a.right - a.left - w) / 2, a.top + ((a.bottom - a.top) as f32 * 0.24) as i32);
            }
            Ok(Box::new(Toast { title, detail, pal, start: Instant::now() }))
        }),
    );
}

impl eframe::App for Toast {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        self.pal.panel.to_normalized_gamma_f32()
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let pal = self.pal;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(16, 10))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 13.0, pal.accent);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, ic::CHECK, icon_font(13.0), pal.on_accent);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new(&self.title).font(semi(15.0)).color(pal.text)).truncate());
                    if !self.detail.is_empty() {
                        ui.label(RichText::new(&self.detail).size(12.0).color(pal.dim));
                    }
                });
            });
        });
        let t = self.start.elapsed().as_secs_f32();
        if t > 1.8 || ui.input(|i| i.pointer.any_click()) {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}
