//! The Servers page: a searchable history of servers each account has joined,
//! read from the Roblox client logs. A new feature over the Python app.

use crate::roblox::launch::LaunchRequest;
use crate::store::model::ServerVisit;
use crate::ui::widgets;
use crate::ui::NovaApp;
use eframe::egui::{self, RichText};

impl NovaApp {
    pub(in crate::ui) fn page_servers(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Servers").size(22.0).strong().color(self.palette.text));
            ui.label(RichText::new("recently joined").size(13.0).color(self.palette.faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::ghost_button(ui, &self.palette, &format!("{} Refresh", egui_phosphor::regular::ARROWS_CLOCKWISE)).clicked() {
                    crate::services::history::sync(&self.core);
                }
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(format!("{}  Filter", egui_phosphor::regular::MAGNIFYING_GLASS)).desired_width(200.0));
            });
        });
        ui.add_space(10.0);

        let mut visits = self.core.server_history();
        visits.reverse(); // newest first
        let terms: Vec<String> = self.search.to_lowercase().split_whitespace().map(str::to_owned).collect();
        let filtered: Vec<ServerVisit> = visits
            .into_iter()
            .filter(|v| {
                if terms.is_empty() {
                    return true;
                }
                let hay = format!("{} {} {} {}", v.username, v.place_id, v.job_id, v.server_ip).to_lowercase();
                terms.iter().all(|t| hay.contains(t))
            })
            .collect();

        if filtered.is_empty() {
            widgets::empty_state(ui, &self.palette, egui_phosphor::regular::GLOBE_HEMISPHERE_WEST, "No server history yet", "Launch a game and it will appear here.");
            return;
        }

        let mut rejoin: Option<ServerVisit> = None;
        let mut copy: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("servers").auto_shrink([false, false]).show(ui, |ui| {
            for visit in &filtered {
                widgets::card(ui, &self.palette, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            let title = self.game_names.get(&visit.place_id).cloned().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Place {}", visit.place_id));
                            ui.label(RichText::new(title).size(14.0).strong().color(self.palette.text));
                            ui.horizontal(|ui| {
                                if !visit.username.is_empty() {
                                    widgets::chip(ui, &visit.username, self.palette.muted, self.palette.surface_alt);
                                }
                                if let Some(at) = visit.at {
                                    ui.label(RichText::new(at.with_timezone(&chrono::Local).format("%b %d, %H:%M").to_string()).size(11.0).color(self.palette.faint));
                                }
                                if !visit.server_ip.is_empty() {
                                    ui.label(RichText::new(&visit.server_ip).size(11.0).color(self.palette.faint));
                                }
                            });
                            ui.label(RichText::new(format!("Job {}", short_job(&visit.job_id))).size(10.0).monospace().color(self.palette.faint));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::ghost_button(ui, &self.palette, &format!("{} Rejoin", egui_phosphor::regular::PLAY)).clicked() {
                                rejoin = Some(visit.clone());
                            }
                            if ui.button(egui_phosphor::regular::COPY).on_hover_text("Copy Job ID").clicked() {
                                copy = Some(visit.job_id.clone());
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
        });

        if let Some(visit) = rejoin {
            self.launch.place_id = visit.place_id.to_string();
            self.launch.job_id = visit.job_id.clone();
            self.launch.private_server.clear();
            // Rejoin with the account that originally joined this server.
            if let Some(account) = self.accounts.iter().find(|a| a.user_id == visit.user_id) {
                self.selection.clear();
                self.selection.insert(account.key());
            }
            let request = LaunchRequest { place_id: visit.place_id.to_string(), job_id: visit.job_id, private_server: String::new() };
            self.launch_selected(request, "Rejoined");
        }
        if let Some(job) = copy {
            ui.ctx().copy_text(job);
            self.show_toast("Job ID copied", false);
        }
    }
}

fn short_job(job: &str) -> String {
    if job.len() > 13 { format!("{}…", &job[..12]) } else { job.to_owned() }
}
