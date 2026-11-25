//! The app icon. The drawing lives in `icon_art.rs` so build.rs can bake the same picture into the exe.

include!("icon_art.rs");

pub fn egui_icon() -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: rgba(64, false), width: 64, height: 64 }
}
