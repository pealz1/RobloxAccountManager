//! The Private Servers page: list an account's private servers, refresh join links,
//! save them to the library and launch into them.

use crate::error::AppResult;
use crate::store::model::SavedPrivateServer;
use crate::ui::NovaApp;
use crate::ui::task::Msg;
use crate::ui::widgets;
use eframe::egui::{self, RichText};
use std::sync::Arc;

#[derive(Default)]
pub struct PrivateServersState {
    pub account: String,
    pub place_filter: String,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded: Vec<SavedPrivateServer>,
}

impl PrivateServersState {
    pub fn on_loaded(&mut self, result: AppResult<Vec<SavedPrivateServer>>) {
        self.loading = false;
        match result {
            Ok(servers) => {
                self.error = None;
                self.loaded = servers;
            }
            Err(err) => self.error = Some(err.message),
        }
    }
}

impl NovaApp {
    pub(in crate::ui) fn page_private_servers(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Private Servers").size(22.0).strong().color(self.palette.text));
        ui.add_space(8.0);

        // Account picker + place filter + load.
        if self.ps_state.account.is_empty()
            && let Some(first) = self.accounts.first()
        {
            self.ps_state.account = first.key();
        }
        let accounts = self.accounts.clone();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("ps-account").selected_text(self.account_label(&self.ps_state.account)).show_ui(ui, |ui| {
                for account in &accounts {
                    let key = account.key();
                    ui.selectable_value(&mut self.ps_state.account, key, account.label());
                }
            });
            ui.add(egui::TextEdit::singleline(&mut self.ps_state.place_filter).hint_text("Place ID (optional)").desired_width(160.0));
            if widgets::primary_button(ui, &self.palette, &format!("{} Load", egui_phosphor::regular::ARROWS_CLOCKWISE)).clicked() {
                self.load_private_servers();
            }
        });
        ui.add_space(10.0);

        if self.ps_state.loading {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Loading servers…").color(self.palette.muted));
            });
            return;
        }
        if let Some(error) = self.ps_state.error.clone() {
            ui.label(RichText::new(error).color(self.palette.danger));
        }

        // Saved servers from the library, plus freshly loaded ones.
        let saved = self.core.private_servers();
        let loaded = self.ps_state.loaded.clone();
        let mut action: Option<Action> = None;

        egui::ScrollArea::vertical().id_salt("ps").auto_shrink([false, false]).show(ui, |ui| {
            if !loaded.is_empty() {
                widgets::section(ui, &self.palette, "Loaded from account");
                for server in &loaded {
                    self.server_row(ui, server, &mut action, false);
                }
                ui.add_space(8.0);
            }
            widgets::section(ui, &self.palette, "Saved");
            if saved.is_empty() {
                widgets::empty_state(
                    ui,
                    &self.palette,
                    egui_phosphor::regular::LOCK_KEY,
                    "No saved private servers",
                    "Load an account's servers and save the ones you use.",
                );
            }
            for server in &saved {
                self.server_row(ui, server, &mut action, true);
            }
        });

        if let Some(action) = action {
            self.run_server_action(action);
        }
    }

    fn account_label(&self, key: &str) -> String {
        self.accounts.iter().find(|a| a.key() == key).map(|a| a.label().to_owned()).unwrap_or_else(|| "Select account".into())
    }

    fn server_row(&self, ui: &mut egui::Ui, server: &SavedPrivateServer, action: &mut Option<Action>, saved: bool) {
        widgets::card(ui, &self.palette, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&server.name).size(14.0).strong().color(self.palette.text));
                    ui.label(
                        RichText::new(format!("{} · place {}", server.game_name, server.place_id)).size(11.0).color(self.palette.faint),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::ghost_button(ui, &self.palette, &format!("{} Join", egui_phosphor::regular::PLAY)).clicked() {
                        *action = Some(Action::Join(server.clone()));
                    }
                    if !server.link.is_empty() && ui.button(egui_phosphor::regular::LINK).on_hover_text("Copy link").clicked() {
                        ui.ctx().copy_text(server.link.clone());
                    }
                    if ui.button(egui_phosphor::regular::ARROWS_CLOCKWISE).on_hover_text("New join link").clicked() {
                        *action = Some(Action::Refresh(server.clone()));
                    }
                    if saved {
                        if ui
                            .button(RichText::new(egui_phosphor::regular::TRASH).color(self.palette.danger))
                            .on_hover_text("Remove")
                            .clicked()
                        {
                            *action = Some(Action::Remove(server.id.clone()));
                        }
                    } else if widgets::ghost_button(ui, &self.palette, &format!("{} Save", egui_phosphor::regular::BOOKMARK_SIMPLE))
                        .clicked()
                    {
                        *action = Some(Action::Save(server.clone()));
                    }
                });
            });
        });
        ui.add_space(6.0);
    }

    fn load_private_servers(&mut self) {
        let Some(account) = self.core.account(&self.ps_state.account) else {
            self.show_toast("Pick an account first", true);
            return;
        };
        let place_filter = self.ps_state.place_filter.trim().parse::<u64>().ok();
        self.ps_state.loading = true;
        self.ps_state.error = None;
        self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |_, _| {
            let result = crate::roblox::private_servers::list_servers(&account.cookie, account.user_id, place_filter);
            Msg::PrivateServers(result)
        });
    }

    fn run_server_action(&mut self, action: Action) {
        match action {
            Action::Join(server) => {
                let keys = if self.selection.is_empty() { vec![self.ps_state.account.clone()] } else { self.selected_keys() };
                let link = if server.link.is_empty() { server.id.clone() } else { server.link.clone() };
                self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |core, _| {
                    match core.join_private_server(&keys, &link) {
                        Ok(result) => Msg::Toast(result.summary("Joined"), !result.ok()),
                        Err(err) => Msg::Toast(err.message, true),
                    }
                });
                self.show_toast("Joining private server…", false);
            }
            Action::Save(server) => match self.core.save_private_servers(vec![server]) {
                Ok(()) => self.show_toast("Saved", false),
                Err(err) => self.show_toast(err.message, true),
            },
            Action::Remove(id) => {
                let _ = self.core.remove_private_server(&id);
            }
            Action::Refresh(server) => {
                let Some(account) = self.core.account(&self.ps_state.account) else { return };
                let place = server.place_id;
                self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |_, _| {
                    match crate::roblox::private_servers::refresh_link(&account.cookie, server.vip_server_id, place) {
                        Ok(link) => Msg::Toast(format!("New link: {link}"), false),
                        Err(err) => Msg::Toast(err.message, true),
                    }
                });
            }
        }
    }
}

enum Action {
    Join(SavedPrivateServer),
    Save(SavedPrivateServer),
    Remove(String),
    Refresh(SavedPrivateServer),
}
