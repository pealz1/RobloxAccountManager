//! The Multi Roblox page: enable multiple clients and manage running instances.

use crate::store::settings::MultiMethod;
use crate::ui::NovaApp;
use crate::ui::widgets;
use eframe::egui::{self, RichText};

impl NovaApp {
    pub(in crate::ui) fn page_multi(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        ui.label(RichText::new("Multi Roblox").size(22.0).strong().color(self.palette.text));
        ui.add_space(10.0);

        let running = self.core.multi_roblox_running();
        let mut settings = self.core.settings();

        widgets::card(ui, &palette, |ui| {
            widgets::setting_row(ui, &palette, "Multi Roblox", if running { "Running" } else { "Off" }, |ui| {
                let mut on = running;
                if ui.add(egui::Checkbox::without_text(&mut on)).changed() {
                    if on {
                        match self.core.enable_multi_roblox() {
                            Ok(()) => self.show_toast("Multi Roblox enabled", false),
                            Err(err) => self.show_toast(err.message, true),
                        }
                    } else {
                        self.core.disable_multi_roblox();
                        self.show_toast("Multi Roblox disabled", false);
                    }
                }
            });

            widgets::setting_row(ui, &palette, "Method", "Mutex works with no admin; Handle works with clients already open", |ui| {
                egui::ComboBox::from_id_salt("multi-method")
                    .selected_text(match settings.multi_method {
                        MultiMethod::Mutex => "Mutex (default)",
                        MultiMethod::Handle => "Handle (admin)",
                    })
                    .show_ui(ui, |ui| {
                        let mut changed = false;
                        changed |= ui.selectable_value(&mut settings.multi_method, MultiMethod::Mutex, "Mutex (default)").changed();
                        changed |= ui.selectable_value(&mut settings.multi_method, MultiMethod::Handle, "Handle (admin)").changed();
                        if changed {
                            let method = settings.multi_method;
                            let _ = self.core.edit_settings(move |s| s.multi_method = method);
                        }
                    });
            });

            widgets::setting_row(ui, &palette, "Error 773 fix", "Lock RobloxCookies.dat while multi-instance is on (Mutex mode)", |ui| {
                if ui.add(egui::Checkbox::without_text(&mut settings.cookie_lock_773)).changed() {
                    let v = settings.cookie_lock_773;
                    let _ = self.core.edit_settings(move |s| s.cookie_lock_773 = v);
                }
            });
        });

        ui.add_space(14.0);
        self.instances_section(ui);
    }

    fn instances_section(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Running clients").size(15.0).strong().color(self.palette.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::ghost_button(ui, &palette, &format!("{} Grid", egui_phosphor::regular::GRID_FOUR)).clicked() {
                    match crate::win::grid::tile_roblox_windows() {
                        Ok(result) => self.show_toast(format!("Arranged into {}×{}", result.columns, result.rows), false),
                        Err(err) => self.show_toast(err.message, true),
                    }
                }
            });
        });
        ui.add_space(6.0);

        let instances = self.services.live.instances();
        if instances.is_empty() {
            widgets::empty_state(
                ui,
                &palette,
                egui_phosphor::regular::STACK,
                "No Roblox clients running",
                "Launch accounts to see them here.",
            );
            return;
        }
        let names: std::collections::HashMap<u64, String> = self.accounts.iter().map(|a| (a.user_id, a.label().to_owned())).collect();
        let mut hide: Option<u32> = None;
        let mut show: Option<u32> = None;
        let mut close: Option<u32> = None;
        egui::ScrollArea::vertical().id_salt("instances").auto_shrink([false, false]).show(ui, |ui| {
            for inst in &instances {
                widgets::card(ui, &palette, |ui| {
                    ui.horizontal(|ui| {
                        let name = names.get(&inst.user_id).cloned().unwrap_or_else(|| format!("User {}", inst.user_id));
                        ui.label(RichText::new(name).size(13.0).strong().color(self.palette.text));
                        ui.label(RichText::new(format!("PID {}", inst.pid)).size(11.0).color(self.palette.faint));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(RichText::new(egui_phosphor::regular::X).color(self.palette.danger))
                                .on_hover_text("Close")
                                .clicked()
                            {
                                close = Some(inst.pid);
                            }
                            if ui.button(egui_phosphor::regular::EYE_SLASH).on_hover_text("Hide").clicked() {
                                hide = Some(inst.pid);
                            }
                            if ui.button(egui_phosphor::regular::EYE).on_hover_text("Show").clicked() {
                                show = Some(inst.pid);
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
        });
        if let Some(pid) = hide {
            crate::services::instances::hide(pid);
        }
        if let Some(pid) = show {
            crate::services::instances::show(pid);
        }
        if let Some(pid) = close {
            crate::services::instances::close(pid);
        }
    }
}
