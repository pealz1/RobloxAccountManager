//! The Accounts page: the account list, group filter, search, launch bar and actions.

use crate::roblox::launch::LaunchRequest;
use crate::store::model::{Account, CookieStatus};
use crate::ui::task::Msg;
use crate::ui::widgets::{self};
use crate::ui::{NovaApp, Page};
use eframe::egui::{self, Color32, RichText, Sense, Vec2};
use std::sync::Arc;

impl NovaApp {
    pub(in crate::ui) fn page_accounts(&mut self, ui: &mut egui::Ui) {
        self.header_bar(ui);
        self.group_bar(ui);
        ui.add_space(8.0);

        let available = ui.available_height();
        ui.horizontal_top(|ui| {
            let list_width = (ui.available_width() - 300.0).max(360.0);
            ui.allocate_ui(Vec2::new(list_width, available), |ui| self.account_list(ui));
            ui.add_space(12.0);
            ui.allocate_ui(Vec2::new(ui.available_width(), available), |ui| self.launch_panel(ui));
        });
    }

    fn header_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Accounts").size(22.0).strong().color(self.palette.text));
            ui.label(RichText::new(format!("{}", self.accounts.len())).size(13.0).color(self.palette.faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary_button(ui, &self.palette, &format!("{}  Add account", egui_phosphor::regular::PLUS)).clicked() {
                    self.add_dialog = Some(super::add_account::AddDialog::default());
                }
                let search = egui::TextEdit::singleline(&mut self.search)
                    .hint_text(format!("{}  Search", egui_phosphor::regular::MAGNIFYING_GLASS))
                    .desired_width(220.0);
                ui.add(search);
            });
        });
        ui.add_space(6.0);
    }

    fn group_bar(&mut self, ui: &mut egui::Ui) {
        let groups = self.core.groups();
        egui::ScrollArea::horizontal().id_salt("groups").max_height(32.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                let all_selected = self.group_filter.is_none();
                if self.group_tab(ui, "All", all_selected).clicked() {
                    self.group_filter = None;
                }
                for group in &groups {
                    let selected = self.group_filter.as_deref() == Some(group.as_str());
                    let response = self.group_tab(ui, group, selected);
                    if response.clicked() {
                        self.group_filter = Some(group.clone());
                    }
                    response.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            self.renaming_group = Some((group.clone(), group.clone()));
                            ui.close();
                        }
                        if ui.button(RichText::new("Delete group").color(self.palette.danger)).clicked() {
                            let name = group.clone();
                            let _ = self.core.delete_group(&name);
                            if self.group_filter.as_deref() == Some(name.as_str()) {
                                self.group_filter = None;
                            }
                            self.reload_accounts();
                            ui.close();
                        }
                    });
                }
                if ui.add(egui::Button::new(RichText::new(egui_phosphor::regular::PLUS).color(self.palette.muted)).fill(Color32::TRANSPARENT)).clicked() {
                    self.group_input = "New Group".into();
                    self.renaming_group = Some((String::new(), "New Group".into()));
                }
            });
        });
        self.group_rename_popup(ui);
    }

    fn group_tab(&self, ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
        let (fg, bg) = if selected { (self.palette.accent_text, self.palette.accent) } else { (self.palette.muted, self.palette.surface_alt) };
        ui.add(egui::Button::new(RichText::new(label).size(12.5).color(fg)).fill(bg).corner_radius(14.0))
    }

    fn group_rename_popup(&mut self, ui: &mut egui::Ui) {
        let Some((original, _)) = self.renaming_group.clone() else { return };
        let mut keep = true;
        egui::Window::new(if original.is_empty() { "New group" } else { "Rename group" })
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                if let Some((_, name)) = self.renaming_group.as_mut() {
                    ui.add(egui::TextEdit::singleline(name).desired_width(200.0));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::primary_button(ui, &self.palette, "Save").clicked() {
                        if let Some((_, name)) = self.renaming_group.clone() {
                            let result = if original.is_empty() { self.core.create_group(&name).map(|_| ()) } else { self.core.rename_group(&original, &name).map(|_| ()) };
                            match result {
                                Ok(()) => self.reload_accounts(),
                                Err(err) => self.show_toast(err.message, true),
                            }
                        }
                        keep = false;
                    }
                    if widgets::ghost_button(ui, &self.palette, "Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        if !keep {
            self.renaming_group = None;
        }
    }

    fn account_list(&mut self, ui: &mut egui::Ui) {
        let accounts = self.visible_accounts();
        if accounts.is_empty() {
            if self.accounts.is_empty() {
                widgets::empty_state(ui, &self.palette, egui_phosphor::regular::USERS_THREE, "No accounts yet", "Add one with the button above.");
            } else {
                widgets::empty_state(ui, &self.palette, egui_phosphor::regular::MAGNIFYING_GLASS, "No matches", "Try a different search or group.");
            }
            return;
        }
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().id_salt("accounts").auto_shrink([false, false]).show(ui, |ui| {
            for account in &accounts {
                self.account_row(ui, &ctx, account);
                ui.add_space(6.0);
            }
        });
    }

    fn account_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, account: &Account) {
        let key = account.key();
        let selected = self.selection.contains(&key);
        let activity = self.services.live.activity_for(account.user_id);
        let texture = self.avatar_texture(ctx, account.user_id);

        let fill = if selected { self.palette.accent.gamma_multiply(0.18) } else { self.palette.surface };
        let stroke = egui::Stroke::new(1.0, if selected { self.palette.accent } else { self.palette.line });
        let response = egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Avatar
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                    if let Some(texture) = &texture {
                        ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    } else {
                        ui.painter().rect_filled(rect, 17.0, self.palette.surface_alt);
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, account.username.chars().next().unwrap_or('?').to_uppercase().to_string(), egui::FontId::proportional(15.0), self.palette.muted);
                    }
                    // Status dot over the avatar
                    let dot = if activity.running { self.palette.in_game } else if account.cookie_status == CookieStatus::Valid { self.palette.online.gamma_multiply(0.5) } else { self.palette.faint };
                    ui.painter().circle_filled(rect.right_bottom() - Vec2::splat(4.0), 4.5, dot);

                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(account.label()).size(14.0).strong().color(self.palette.text));
                            if account.starred {
                                ui.label(RichText::new(egui_phosphor::regular::STAR).size(12.0).color(self.palette.star));
                            }
                            if account.cookie_status == CookieStatus::Invalid {
                                widgets::chip(ui, "invalid", self.palette.accent_text, self.palette.danger);
                            }
                        });
                        ui.horizontal(|ui| {
                            if !account.group.is_empty() {
                                widgets::chip(ui, &account.group, self.palette.muted, self.palette.surface_alt);
                            }
                            let sub = if !account.note.is_empty() { account.note.clone() } else if account.user_id > 0 { format!("ID {}", account.user_id) } else { String::new() };
                            if !sub.is_empty() {
                                ui.label(RichText::new(sub).size(11.0).color(self.palette.faint));
                            }
                        });
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if activity.running {
                            let ram = if activity.ram_mb >= 1024.0 { format!("{:.1} GB", activity.ram_mb / 1024.0) } else { format!("{:.0} MB", activity.ram_mb) };
                            ui.label(RichText::new(ram).size(11.0).color(self.palette.muted));
                            widgets::status_dot(ui, self.palette.in_game);
                        }
                    });
                });
            })
            .response
            .interact(Sense::click());

        if response.clicked() {
            self.handle_row_click(ui, &key);
        }
        response.context_menu(|ui| self.account_context_menu(ui, account));
    }

    fn handle_row_click(&mut self, ui: &egui::Ui, key: &str) {
        let modifiers = ui.input(|i| i.modifiers);
        if modifiers.ctrl || modifiers.command {
            if !self.selection.remove(key) {
                self.selection.insert(key.to_owned());
            }
        } else if modifiers.shift && self.last_clicked.is_some() {
            let order: Vec<String> = self.visible_accounts().iter().map(Account::key).collect();
            let anchor = self.last_clicked.clone().unwrap();
            if let (Some(a), Some(b)) = (order.iter().position(|k| k == &anchor), order.iter().position(|k| k == key)) {
                let (lo, hi) = (a.min(b), a.max(b));
                for k in &order[lo..=hi] {
                    self.selection.insert(k.clone());
                }
            }
        } else {
            self.selection.clear();
            self.selection.insert(key.to_owned());
        }
        self.last_clicked = Some(key.to_owned());
    }

    fn account_context_menu(&mut self, ui: &mut egui::Ui, account: &Account) {
        let key = account.key();
        if !self.selection.contains(&key) {
            self.selection.clear();
            self.selection.insert(key.clone());
            self.last_clicked = Some(key.clone());
        }
        let count = self.selection.len();
        if ui.button(format!("{}  Launch home", egui_phosphor::regular::HOUSE)).clicked() {
            self.launch_selected(LaunchRequest::default(), "Launched");
            ui.close();
        }
        if ui.button(format!("{}  Join place", egui_phosphor::regular::PLAY)).clicked() {
            self.launch_selected(self.current_request(), "Joined");
            ui.close();
        }
        ui.separator();
        let star = account.starred;
        if ui.button(if star { "Unstar" } else { "Star" }).clicked() {
            let _ = self.core.set_starred(&key, !star);
            self.reload_accounts();
            ui.close();
        }
        ui.menu_button(format!("{}  Move to group", egui_phosphor::regular::FOLDER), |ui| {
            if ui.button("(none)").clicked() {
                for k in self.selected_keys() {
                    let _ = self.core.set_group(&k, "");
                }
                self.reload_accounts();
                ui.close();
            }
            for group in self.core.groups() {
                if ui.button(&group).clicked() {
                    for k in self.selected_keys() {
                        let _ = self.core.set_group(&k, &group);
                    }
                    self.reload_accounts();
                    ui.close();
                }
            }
        });
        if ui.button(format!("{}  Edit note", egui_phosphor::regular::NOTE_PENCIL)).clicked() {
            self.add_dialog = Some(super::add_account::AddDialog::note(account));
            ui.close();
        }
        if count == 1 {
            if self.core.settings().allow_copy_secrets {
                if ui.button(format!("{}  Copy cookie", egui_phosphor::regular::COPY)).clicked() {
                    ui.ctx().copy_text(account.cookie.clone());
                    self.show_toast("Cookie copied", false);
                    ui.close();
                }
            }
            if ui.button(format!("{}  Refresh cookie status", egui_phosphor::regular::ARROWS_CLOCKWISE)).clicked() {
                let k = key.clone();
                self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |core, _| {
                    crate::services::cookie_check::check_one(core, &k);
                    Msg::AccountsChanged
                });
                ui.close();
            }
        }
        ui.separator();
        if ui.button(RichText::new(format!("{}  Delete ({count})", egui_phosphor::regular::TRASH)).color(self.palette.danger)).clicked() {
            let keys = self.selected_keys();
            self.confirm = Some(super::super::Confirm {
                title: format!("Delete {count} account(s)?"),
                body: "They are removed from Nova only. The Roblox accounts are not affected.".into(),
                action: Box::new(move |app| {
                    for k in &keys {
                        let _ = app.core.delete_account(k);
                    }
                    app.reload_accounts();
                    app.show_toast(format!("Deleted {} account(s)", keys.len()), false);
                }),
            });
            ui.close();
        }
    }

    // ---- Launch bar ----

    pub(in crate::ui) fn current_request(&self) -> LaunchRequest {
        LaunchRequest {
            place_id: self.launch.place_id.trim().to_owned(),
            private_server: self.launch.private_server.trim().to_owned(),
            job_id: self.launch.job_id.trim().to_owned(),
        }
    }

    fn launch_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            ui.label(RichText::new("Launch").size(15.0).strong().color(self.palette.text));
            ui.add_space(8.0);

            ui.label(RichText::new("Place ID").size(11.0).color(self.palette.muted));
            let place_changed = ui.add(egui::TextEdit::singleline(&mut self.launch.place_id).hint_text("e.g. 606849621").desired_width(f32::INFINITY)).changed();
            if place_changed {
                self.launch.game_name.clear();
            }
            if !self.launch.game_name.is_empty() {
                ui.label(RichText::new(&self.launch.game_name).size(11.0).color(self.palette.accent));
            } else if let Ok(place) = self.launch.place_id.trim().parse::<u64>() {
                self.request_game_name(place);
            }

            ui.add_space(6.0);
            ui.label(RichText::new("Private server link").size(11.0).color(self.palette.muted));
            ui.add(egui::TextEdit::singleline(&mut self.launch.private_server).hint_text("VIP / share link or code").desired_width(f32::INFINITY));

            ui.add_space(6.0);
            ui.label(RichText::new("Job ID (optional)").size(11.0).color(self.palette.muted));
            ui.add(egui::TextEdit::singleline(&mut self.launch.job_id).hint_text("specific server").desired_width(f32::INFINITY));

            ui.add_space(10.0);
            let selected = self.selection.len();
            ui.label(RichText::new(format!("{selected} selected")).size(11.0).color(self.palette.faint));
            ui.add_space(4.0);

            let full = ui.available_width();
            if ui.add(egui::Button::new(RichText::new(format!("{}  Join", egui_phosphor::regular::PLAY)).color(self.palette.accent_text).strong()).fill(self.palette.accent).corner_radius(8.0).min_size(Vec2::new(full, 34.0))).clicked() {
                self.launch_selected(self.current_request(), "Joined");
            }
            ui.add_space(6.0);
            egui::Grid::new("launch-actions").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
                if widgets::ghost_button(ui, &self.palette, &format!("{} Home", egui_phosphor::regular::HOUSE)).clicked() {
                    self.launch_selected(LaunchRequest::default(), "Launched");
                }
                if widgets::ghost_button(ui, &self.palette, &format!("{} Small server", egui_phosphor::regular::USERS)).clicked() {
                    self.join_small_server();
                }
                if widgets::ghost_button(ui, &self.palette, &format!("{} Save favourite", egui_phosphor::regular::STAR)).clicked() {
                    self.save_current_favorite();
                }
                if widgets::ghost_button(ui, &self.palette, &format!("{} Servers", egui_phosphor::regular::LIST)).clicked() {
                    self.page = Page::PrivateServers;
                }
                ui.end_row();
            });

            ui.add_space(12.0);
            ui.label(RichText::new("Join a user").size(11.0).color(self.palette.muted));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.launch.join_user).hint_text("username / id").desired_width(ui.available_width() - 44.0));
                if ui.button(egui_phosphor::regular::ARROW_RIGHT).clicked() {
                    self.join_user_action();
                }
            });

            self.favorites_dropdown(ui);
            self.recent_games_list(ui);
        });
    }

    fn favorites_dropdown(&mut self, ui: &mut egui::Ui) {
        let favorites = self.core.favorites();
        if favorites.is_empty() {
            return;
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Favourites").size(11.0).color(self.palette.muted));
        egui::ComboBox::from_id_salt("favorites").selected_text("Pick a favourite").width(ui.available_width()).show_ui(ui, |ui| {
            for fav in favorites {
                if ui.selectable_label(false, format!("{} ({})", fav.name, fav.place_id)).clicked() {
                    self.launch.place_id = fav.place_id.to_string();
                    self.launch.private_server = fav.private_server.clone();
                    self.launch.game_name = fav.name.clone();
                }
            }
        });
    }

    fn recent_games_list(&mut self, ui: &mut egui::Ui) {
        let recents = self.core.recent_games();
        if recents.is_empty() {
            return;
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Recent").size(11.0).color(self.palette.muted));
        for game in recents.into_iter().take(5) {
            let label = format!("{}{}", if game.private_server.is_empty() { "" } else { "[P] " }, if game.name.is_empty() { game.place_id.to_string() } else { game.name.clone() });
            if ui.add(egui::Button::new(RichText::new(label).size(12.0).color(self.palette.text)).fill(Color32::TRANSPARENT)).clicked() {
                self.launch.place_id = game.place_id.to_string();
                self.launch.private_server = game.private_server.clone();
                self.launch.game_name = game.name.clone();
            }
        }
    }

    fn save_current_favorite(&mut self) {
        let Ok(place_id) = self.launch.place_id.trim().parse::<u64>() else {
            self.show_toast("Enter a Place ID first", true);
            return;
        };
        let name = if self.launch.game_name.is_empty() { place_id.to_string() } else { self.launch.game_name.clone() };
        let fav = crate::store::model::Favorite { place_id, name, private_server: self.launch.private_server.trim().to_owned() };
        match self.core.add_favorite(fav) {
            Ok(()) => self.show_toast("Saved to favourites", false),
            Err(err) => self.show_toast(err.message, true),
        }
    }

    // ---- Launch actions (run off the UI thread) ----

    pub(in crate::ui) fn launch_selected(&mut self, request: LaunchRequest, verb: &'static str) {
        let keys = self.selected_keys();
        if keys.is_empty() {
            self.show_toast("Select at least one account", true);
            return;
        }
        let count = keys.len();
        self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |core, _| {
            let result = core.launch_batch(&keys, &request);
            Msg::Toast(result.summary(verb), !result.ok())
        });
        self.show_toast(format!("{verb} {count}…"), false);
    }

    fn join_small_server(&mut self) {
        let keys = self.selected_keys();
        let Ok(place_id) = self.launch.place_id.trim().parse::<u64>() else {
            self.show_toast("Enter a Place ID first", true);
            return;
        };
        if keys.is_empty() {
            self.show_toast("Select at least one account", true);
            return;
        }
        self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |core, _| match core.join_small_server(&keys, place_id) {
            Ok(result) => Msg::Toast(result.summary("Joined"), !result.ok()),
            Err(err) => Msg::Toast(err.message, true),
        });
    }

    fn join_user_action(&mut self) {
        let keys = self.selected_keys();
        let target = self.launch.join_user.trim().to_owned();
        if target.is_empty() || keys.is_empty() {
            self.show_toast("Select accounts and enter a user", true);
            return;
        }
        self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |core, _| match core.join_user(&keys, &target) {
            Ok(result) => Msg::Toast(result.summary("Joined"), !result.ok()),
            Err(err) => Msg::Toast(err.message, true),
        });
    }

    fn request_game_name(&mut self, place: u64) {
        if self.game_names.contains_key(&place) {
            self.launch.game_name = self.game_names[&place].clone();
            return;
        }
        if self.game_name_pending.insert(place) {
            self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |_, _| {
                let name = crate::roblox::games::game_name(place).unwrap_or_default();
                Msg::GameName(place, name)
            });
        }
    }
}
