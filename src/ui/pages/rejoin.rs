//! The Auto-Rejoin page: configure and run per-account rejoin workers.

use crate::store::model::RejoinConfig;
use crate::ui::widgets;
use crate::ui::NovaApp;
use eframe::egui::{self, RichText};
use std::sync::Arc;

pub struct RejoinDialog {
    pub editing: bool,
    pub config: RejoinConfig,
    pub place_text: String,
}

impl RejoinDialog {
    fn new(account: String) -> RejoinDialog {
        RejoinDialog { editing: false, config: RejoinConfig { account, ..Default::default() }, place_text: String::new() }
    }

    fn from_config(config: RejoinConfig) -> RejoinDialog {
        let place_text = if config.place_id == 0 { String::new() } else { config.place_id.to_string() };
        RejoinDialog { editing: true, config, place_text }
    }
}

impl NovaApp {
    pub(in crate::ui) fn page_auto_rejoin(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Auto-Rejoin").size(22.0).strong().color(self.palette.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::ghost_button(ui, &self.palette, &format!("{} Stop all", egui_phosphor::regular::STOP)).clicked() {
                    self.rejoin.stop_all();
                }
                if widgets::primary_button(ui, &self.palette, &format!("{} Add", egui_phosphor::regular::PLUS)).clicked() {
                    let account = self.selected_keys().first().cloned().or_else(|| self.accounts.first().map(|a| a.key())).unwrap_or_default();
                    self.rejoin_dialog = Some(RejoinDialog::new(account));
                }
            });
        });
        ui.label(RichText::new("Relaunches an account when its client closes or it leaves the target place.").size(12.0).color(self.palette.muted));
        ui.add_space(10.0);

        let configs = self.core.snapshot().auto_rejoin.clone();
        if configs.is_empty() {
            widgets::empty_state(ui, &self.palette, egui_phosphor::regular::ARROWS_CLOCKWISE, "No Auto-Rejoin entries", "Add one to keep an account in a game.");
            return;
        }

        let mut to_start: Option<RejoinConfig> = None;
        let mut to_stop: Option<String> = None;
        let mut to_edit: Option<RejoinConfig> = None;
        let mut to_remove: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("rejoin").auto_shrink([false, false]).show(ui, |ui| {
            for config in &configs {
                let running = self.rejoin.is_running(&config.account);
                let status = self.services.live.rejoin_status(&config.account);
                widgets::card(ui, &self.palette, |ui| {
                    ui.horizontal(|ui| {
                        widgets::status_dot(ui, if running { self.palette.online } else { self.palette.faint });
                        ui.vertical(|ui| {
                            ui.label(RichText::new(self.account_label_for(&config.account)).size(14.0).strong().color(self.palette.text));
                            let sub = status.unwrap_or_else(|| format!("Place {}", config.place_id));
                            ui.label(RichText::new(sub).size(11.0).color(self.palette.faint));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new(egui_phosphor::regular::TRASH).color(self.palette.danger)).clicked() {
                                to_remove = Some(config.account.clone());
                            }
                            if ui.button(egui_phosphor::regular::PENCIL_SIMPLE).clicked() {
                                to_edit = Some(config.clone());
                            }
                            if running {
                                if widgets::ghost_button(ui, &self.palette, &format!("{} Stop", egui_phosphor::regular::STOP)).clicked() {
                                    to_stop = Some(config.account.clone());
                                }
                            } else if widgets::primary_button(ui, &self.palette, &format!("{} Start", egui_phosphor::regular::PLAY)).clicked() {
                                to_start = Some(config.clone());
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
        });

        if let Some(config) = to_start {
            self.rejoin.start(&self.services, config);
        }
        if let Some(account) = to_stop {
            self.rejoin.stop(&account);
        }
        if let Some(config) = to_edit {
            self.rejoin_dialog = Some(RejoinDialog::from_config(config));
        }
        if let Some(account) = to_remove {
            self.rejoin.stop(&account);
            let _ = self.core.edit(|data| data.auto_rejoin.retain(|c| c.account != account));
        }
    }

    fn account_label_for(&self, key: &str) -> String {
        self.accounts.iter().find(|a| a.key() == key).map(|a| a.label().to_owned()).unwrap_or_else(|| key.to_owned())
    }
}

pub fn show_dialog(app: &mut NovaApp, ctx: &egui::Context, mut dialog: RejoinDialog) -> Option<RejoinDialog> {
    let mut keep = true;
    let accounts = app.accounts.clone();
    egui::Modal::new(egui::Id::new("rejoin-dialog")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.label(RichText::new(if dialog.editing { "Edit Auto-Rejoin" } else { "Add Auto-Rejoin" }).size(16.0).strong());
        ui.add_space(10.0);

        ui.label(RichText::new("Account").size(11.0).color(app.palette.muted));
        egui::ComboBox::from_id_salt("rejoin-account")
            .selected_text(app.account_label_for(&dialog.config.account))
            .show_ui(ui, |ui| {
                for account in &accounts {
                    ui.selectable_value(&mut dialog.config.account, account.key(), account.label());
                }
            });

        ui.add_space(6.0);
        ui.label(RichText::new("Place ID").size(11.0).color(app.palette.muted));
        ui.add(egui::TextEdit::singleline(&mut dialog.place_text).desired_width(f32::INFINITY));

        ui.add_space(6.0);
        ui.label(RichText::new("Private server (optional)").size(11.0).color(app.palette.muted));
        ui.add(egui::TextEdit::singleline(&mut dialog.config.private_server).desired_width(f32::INFINITY));

        ui.add_space(8.0);
        egui::Grid::new("rejoin-opts").num_columns(2).show(ui, |ui| {
            ui.label("Check interval (s)");
            ui.add(egui::DragValue::new(&mut dialog.config.check_interval_secs).range(5..=600));
            ui.end_row();
            ui.label("Max retries");
            ui.add(egui::DragValue::new(&mut dialog.config.max_retries).range(1..=100));
            ui.end_row();
        });
        ui.checkbox(&mut dialog.config.check_presence, "Use presence to detect disconnects");
        ui.checkbox(&mut dialog.config.check_place_id, "Only count being in the target place");
        ui.checkbox(&mut dialog.config.check_internet, "Wait for internet before relaunching");

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, &app.palette, "Save").clicked() {
                dialog.config.place_id = dialog.place_text.trim().parse().unwrap_or(0);
                if dialog.config.account.is_empty() || dialog.config.place_id == 0 {
                    app.show_toast("Pick an account and a Place ID", true);
                } else {
                    let config = dialog.config.clone();
                    let _ = app.core.edit(move |data| {
                        data.auto_rejoin.retain(|c| c.account != config.account);
                        data.auto_rejoin.push(config);
                    });
                    keep = false;
                }
            }
            if widgets::ghost_button(ui, &app.palette, "Cancel").clicked() {
                keep = false;
            }
        });
    });
    let _ = Arc::clone(&app.core);
    keep.then_some(dialog)
}
