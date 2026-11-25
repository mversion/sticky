//! The installer window (sticky-setup.exe): what will happen, a couple of choices, Install.

use super::*;
use crate::settings::Settings;
use crate::setup;
use crate::update::VERSION;
use crate::{kite, win};
use eframe::egui::{self, Align, Color32, Layout, Margin, RichText, Vec2, ViewportCommand};
use std::path::PathBuf;

enum Stage {
    Ready,
    Done(PathBuf, Vec<String>),
    Failed(String),
}

struct Setup {
    pal: Pal,
    hotkey: String,
    logo: egui::TextureHandle,
    installed: Option<String>,
    others: Vec<PathBuf>,
    autostart: bool,
    kite_found: bool,
    kite_existing: Option<String>,
    kite_on: bool,
    kite_kw: String,
    stage: Stage,
}

pub fn run() {
    let settings = Settings::load();
    let vb = egui::ViewportBuilder::default()
        .with_title("Sticky Setup")
        .with_inner_size(Vec2::new(500.0, 430.0))
        .with_resizable(false)
        .with_maximize_button(false);
    let _ = eframe::run_native(
        "Sticky Setup",
        native_options(vb),
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let pal = palette(&settings);
            apply_style(&cc.egui_ctx, &pal, &settings);
            if let Some(h) = win::hwnd_from(cc) {
                let a = win::primary_area();
                let (w, ht) = ((500.0 * a.scale) as i32, (430.0 * a.scale) as i32);
                win::move_window(h, a.left + (a.right - a.left - w) / 2, a.top + (a.bottom - a.top - ht) / 3);
            }
            let img = egui::ColorImage::from_rgba_unmultiplied([128, 128], &crate::icon::rgba(128, false));
            let logo = cc.egui_ctx.load_texture("logo", img, egui::TextureOptions::LINEAR);
            let kite_existing = kite::existing_keyword();
            Ok(Box::new(Setup {
                pal,
                hotkey: settings.hotkey.clone(),
                logo,
                installed: setup::installed_version(),
                others: setup::other_copies(),
                autostart: settings.start_with_windows,
                kite_found: kite::found(),
                kite_on: true,
                kite_kw: kite_existing.clone().unwrap_or_else(|| settings.kite_keyword.clone()),
                kite_existing,
                stage: Stage::Ready,
            }))
        }),
    );
}

fn primary(text: &str, pal: &Pal) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).font(semi(14.0)).color(pal.on_accent)).fill(pal.accent).min_size(Vec2::new(104.0, 34.0))
}

fn secondary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).size(14.0)).min_size(Vec2::new(90.0, 34.0))
}

impl eframe::App for Setup {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        self.pal.bg.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let pal = self.pal;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.bg).inner_margin(Margin::same(24))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new((self.logo.id(), Vec2::splat(52.0))));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Sticky").font(semi(24.0)).color(pal.text));
                    ui.label(RichText::new(format!("Notes, to-dos and reminders · version {VERSION}")).size(13.0).color(pal.dim));
                });
            });
            ui.add_space(18.0);
            let dim = |t: String| RichText::new(t).size(13.0).color(pal.dim);
            match &self.stage {
                Stage::Ready => {
                    let what = match &self.installed {
                        None => "Installs Sticky for you only. No admin rights needed.".to_string(),
                        Some(v) if v == VERSION => format!("Sticky {v} is already installed. Installing again repairs it."),
                        Some(v) => format!("Replaces Sticky {v} with {VERSION}. Your notes and settings stay as they are."),
                    };
                    ui.label(RichText::new(what).size(14.0).color(pal.text));
                    ui.add_space(4.0);
                    ui.label(dim(format!("Folder: {}", setup::install_dir().display())));
                    ui.label(dim("Adds it to the Start menu, and to Settings > Apps to uninstall.".into()));
                    if !self.others.is_empty() {
                        ui.add_space(4.0);
                        let list = self.others.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
                        ui.label(RichText::new(format!("Sticky is running from {list}. It will be closed and the installed copy used instead.")).size(13.0).color(pal.amber));
                    }
                    ui.add_space(14.0);
                    ui.checkbox(&mut self.autostart, RichText::new("Start with Windows (tray icon, hotkey and reminders)").size(14.0));
                    if self.kite_found {
                        ui.add_space(2.0);
                        match &self.kite_existing {
                            Some(kw) => {
                                ui.checkbox(&mut self.kite_on, RichText::new(format!("Point Kite's \"{kw}\" command at the installed copy")).size(14.0));
                            }
                            None => {
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut self.kite_on, RichText::new("Add a Kite command:").size(14.0));
                                    ui.add_enabled(self.kite_on, egui::TextEdit::singleline(&mut self.kite_kw).desired_width(60.0));
                                });
                            }
                        }
                        ui.label(dim("Kite restarts to pick it up.".into()));
                    }
                }
                Stage::Done(_, notes) => {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 12.0, pal.green);
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, ic::CHECK, icon_font(12.0), Color32::WHITE);
                        ui.label(RichText::new("Sticky is installed").font(semi(17.0)).color(pal.text));
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("It's running in the tray. Press {} to add a task or reminder.", self.hotkey)).size(14.0).color(pal.text));
                    for n in notes {
                        ui.label(dim(n.clone()));
                    }
                    if !self.others.is_empty() {
                        ui.add_space(4.0);
                        let dirs = self.others.iter().filter_map(|p| p.parent()).map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
                        ui.label(dim(format!("The old copy in {dirs} is no longer used. You can delete it.")));
                    }
                }
                Stage::Failed(e) => {
                    ui.label(RichText::new("Install didn't finish").font(semi(17.0)).color(pal.red));
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).size(13.0).color(pal.text));
                }
            }

            // buttons along the bottom right
            let bottom = ui.max_rect();
            let area = egui::Rect::from_min_max(egui::pos2(bottom.left(), bottom.bottom() - 34.0), bottom.max);
            ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(Layout::right_to_left(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                match &self.stage {
                    Stage::Ready | Stage::Failed(_) => {
                        let label = match (&self.stage, &self.installed) {
                            (Stage::Failed(_), _) => "Try again",
                            (_, Some(_)) => "Update",
                            _ => "Install",
                        };
                        if ui.add(primary(label, &pal)).clicked() {
                            let o = setup::Options { autostart: self.autostart, kite: (self.kite_found && self.kite_on).then(|| self.kite_kw.trim().to_string()) };
                            self.stage = match setup::install(&o) {
                                Ok((exe, notes)) => Stage::Done(exe, notes),
                                Err(e) => Stage::Failed(e),
                            };
                        }
                        if ui.add(secondary("Cancel")).clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    }
                    Stage::Done(exe, _) => {
                        if ui.add(primary("Open Sticky", &pal)).clicked() {
                            let _ = std::process::Command::new(exe).arg("open").spawn();
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ui.add(secondary("Close")).clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    }
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}
