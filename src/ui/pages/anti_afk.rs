//! The Anti-AFK page: configure the keep-alive action and toggle it on.

use crate::ui::widgets;
use crate::ui::NovaApp;
use eframe::egui::{self, RichText};

const ACTIONS: [&str; 10] = ["space", "w", "a", "s", "d", "up", "down", "left", "right", "shift"];

impl NovaApp {
    pub(in crate::ui) fn page_anti_afk(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Anti-AFK").size(22.0).strong().color(self.palette.text));
        ui.label(RichText::new("Sends an input to each Roblox window on a timer so sessions aren't kicked for inactivity.").size(12.0).color(self.palette.muted));
        ui.add_space(12.0);

        let palette = self.palette;
        let running = self.anti_afk.is_some();
        let mut settings = self.core.settings();
        let mut changed = false;

        widgets::card(ui, &palette, |ui| {
            widgets::setting_row(ui, &palette, "Enabled", "Starts the keep-alive loop for all running clients", |ui| {
                let mut on = running;
                if ui.add(egui::Checkbox::without_text(&mut on)).changed() {
                    if on {
                        self.anti_afk = Some(crate::services::anti_afk::start(settings.anti_afk.clone()));
                        self.show_toast("Anti-AFK started", false);
                    } else {
                        self.anti_afk = None;
                        self.show_toast("Anti-AFK stopped", false);
                    }
                }
            });

            widgets::setting_row(ui, &palette, "Action", "Key pressed in each window", |ui| {
                egui::ComboBox::from_id_salt("afk-action").selected_text(&settings.anti_afk.action).show_ui(ui, |ui| {
                    for action in ACTIONS {
                        if ui.selectable_label(settings.anti_afk.action == action, action).clicked() {
                            settings.anti_afk.action = action.to_owned();
                            changed = true;
                        }
                    }
                });
            });

            widgets::setting_row(ui, &palette, "Presses", "How many times per cycle", |ui| {
                if ui.add(egui::DragValue::new(&mut settings.anti_afk.press_count).range(1..=20)).changed() {
                    changed = true;
                }
            });

            widgets::setting_row(ui, &palette, "Interval", "Minutes between cycles", |ui| {
                if ui.add(egui::DragValue::new(&mut settings.anti_afk.interval_minutes).range(1..=19)).changed() {
                    changed = true;
                }
            });
        });

        if changed {
            let afk = settings.anti_afk.clone();
            let _ = self.core.edit_settings(move |s| s.anti_afk = afk);
        }
    }
}
