//! The Settings page: appearance, launching, accounts, windows, updates, security,
//! data and the local API / MCP server.

use crate::store::settings::{Launcher, Settings, ThemeMode, UpdateChannel, WindowTitleMode};
use crate::store::vault::Protection;
use crate::ui::NovaApp;
use crate::ui::task::Msg;
use crate::ui::{theme, widgets};
use eframe::egui::{self, RichText};
use std::sync::Arc;

impl NovaApp {
    pub(in crate::ui) fn page_settings(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        ui.label(RichText::new("Settings").size(22.0).strong().color(self.palette.text));
        ui.add_space(10.0);
        let mut settings = self.core.settings();
        let mut dirty = false;

        egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]).show(ui, |ui| {
            dirty |= self.appearance(ui, &mut settings);
            ui.add_space(12.0);
            dirty |= self.launching(ui, &mut settings);
            ui.add_space(12.0);
            dirty |= self.accounts_section(ui, &mut settings);
            ui.add_space(12.0);
            dirty |= self.windows_section(ui, &mut settings);
            ui.add_space(12.0);
            self.updates_section(ui, &settings);
            ui.add_space(12.0);
            self.security_section(ui);
            ui.add_space(12.0);
            self.data_section(ui);
            ui.add_space(12.0);
            dirty |= self.api_section(ui, &mut settings);
            ui.add_space(12.0);
            self.about_section(ui);
        });

        if dirty {
            let new = settings.clone();
            let _ = self.core.edit_settings(move |s| *s = new);
            self.palette = theme::palette(settings.theme, &settings.accent, self.system_dark);
            theme::apply(ui.ctx(), &palette, settings.ui_scale);
        }
    }

    fn appearance(&self, ui: &mut egui::Ui, s: &mut Settings) -> bool {
        let palette = self.palette;
        let mut dirty = false;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Appearance");
            widgets::setting_row(ui, &palette, "Theme", "", |ui| {
                egui::ComboBox::from_id_salt("theme").selected_text(format!("{:?}", s.theme)).show_ui(ui, |ui| {
                    for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System] {
                        dirty |= ui.selectable_value(&mut s.theme, mode, format!("{mode:?}")).changed();
                    }
                });
            });
            widgets::setting_row(ui, &palette, "Accent colour", "", |ui| {
                let mut color = theme::parse_hex(&s.accent).unwrap_or(self.palette.accent);
                if ui.color_edit_button_srgba(&mut color).changed() {
                    s.accent = theme::to_hex(color);
                    dirty = true;
                }
            });
            widgets::setting_row(ui, &palette, "Interface scale", "", |ui| {
                dirty |= ui.add(egui::Slider::new(&mut s.ui_scale, 0.8..=1.6).step_by(0.05)).changed();
            });
            widgets::setting_row(ui, &palette, "Compact rows", "Tighter account list", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.compact_rows)).changed();
            });
            widgets::setting_row(ui, &palette, "Show avatars", "", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.show_avatars)).changed();
            });
        });
        dirty
    }

    fn launching(&self, ui: &mut egui::Ui, s: &mut Settings) -> bool {
        let palette = self.palette;
        let mut dirty = false;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Launching");
            widgets::setting_row(ui, &palette, "Launcher", "", |ui| {
                egui::ComboBox::from_id_salt("launcher").selected_text(s.launcher.label()).show_ui(ui, |ui| {
                    for launcher in Launcher::ALL {
                        dirty |= ui.selectable_value(&mut s.launcher, launcher, launcher.label()).changed();
                    }
                });
            });
            if s.launcher == Launcher::Custom {
                widgets::setting_row(ui, &palette, "Custom launcher", "", |ui| {
                    if ui.button(egui_phosphor::regular::FOLDER_OPEN).clicked()
                        && let Some(path) = rfd::FileDialog::new().add_filter("Executable", &["exe"]).pick_file()
                    {
                        s.custom_launcher_path = path.display().to_string();
                        dirty = true;
                    }
                    ui.add(egui::TextEdit::singleline(&mut s.custom_launcher_path).desired_width(200.0));
                });
            }
            widgets::setting_row(ui, &palette, "Launch delay", "Seconds between accounts in a batch", |ui| {
                dirty |= ui.add(egui::Slider::new(&mut s.launch_delay_secs, 0.0..=10.0).step_by(0.5)).changed();
            });
            widgets::setting_row(ui, &palette, "Confirm before launch", "Ask before launching many accounts", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.confirm_before_launch)).changed();
            });
        });
        dirty
    }

    fn accounts_section(&self, ui: &mut egui::Ui, s: &mut Settings) -> bool {
        let palette = self.palette;
        let mut dirty = false;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Accounts");
            widgets::setting_row(ui, &palette, "Validate cookies on startup", "", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.validate_cookies_on_startup)).changed();
            });
            widgets::setting_row(ui, &palette, "Validation delay", "Seconds between cookie checks", |ui| {
                dirty |= ui.add(egui::Slider::new(&mut s.cookie_validation_delay_secs, 0.5..=10.0).step_by(0.5)).changed();
            });
            widgets::setting_row(ui, &palette, "Avatar refresh", "Days before re-downloading (0 = keep)", |ui| {
                dirty |= ui.add(egui::DragValue::new(&mut s.avatar_cache_days).range(0..=90)).changed();
            });
            widgets::setting_row(ui, &palette, "Allow copying secrets", "Enables Copy Cookie in the menu", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.allow_copy_secrets)).changed();
            });
        });
        dirty
    }

    fn windows_section(&self, ui: &mut egui::Ui, s: &mut Settings) -> bool {
        let palette = self.palette;
        let mut dirty = false;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Windows & process");
            widgets::setting_row(ui, &palette, "Rename Roblox windows", "Title each window by its account", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.rename_windows)).changed();
            });
            widgets::setting_row(ui, &palette, "Window title", "", |ui| {
                egui::ComboBox::from_id_salt("title-mode").selected_text(format!("{:?}", s.window_title_mode)).show_ui(ui, |ui| {
                    for mode in [WindowTitleMode::Username, WindowTitleMode::Alias, WindowTitleMode::Note, WindowTitleMode::UsernameAndNote]
                    {
                        dirty |= ui.selectable_value(&mut s.window_title_mode, mode, format!("{mode:?}")).changed();
                    }
                });
            });
            widgets::setting_row(ui, &palette, "Activity monitor", "Show running clients and RAM", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.activity_monitor)).changed();
            });
            widgets::setting_row(ui, &palette, "Track server history", "Record joined servers for the Servers page", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.track_server_history)).changed();
            });
            widgets::setting_row(ui, &palette, "Start with Windows", "", |ui| {
                let mut on = crate::win::startup::is_startup_enabled();
                if ui.add(egui::Checkbox::without_text(&mut on)).changed() {
                    let _ = crate::win::startup::set_startup(on);
                }
            });
        });
        dirty
    }

    fn updates_section(&mut self, ui: &mut egui::Ui, s: &Settings) {
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Updates");
            let mut check = s.check_updates;
            widgets::setting_row(ui, &palette, "Check on startup", "", |ui| {
                if ui.add(egui::Checkbox::without_text(&mut check)).changed() {
                    let _ = self.core.edit_settings(move |s| s.check_updates = check);
                }
            });
            let mut channel = s.update_channel;
            widgets::setting_row(ui, &palette, "Channel", "", |ui| {
                egui::ComboBox::from_id_salt("channel").selected_text(format!("{channel:?}")).show_ui(ui, |ui| {
                    let mut changed = false;
                    changed |= ui.selectable_value(&mut channel, UpdateChannel::Stable, "Stable").changed();
                    changed |= ui.selectable_value(&mut channel, UpdateChannel::Beta, "Beta").changed();
                    if changed {
                        let _ = self.core.edit_settings(move |s| s.update_channel = channel);
                    }
                });
            });
            ui.horizontal(|ui| {
                if let Some(version) = self.services.live.update_available() {
                    ui.label(RichText::new(format!("v{version} available")).color(self.palette.accent));
                    if widgets::primary_button(ui, &palette, "Install update").clicked() {
                        self.install_update();
                    }
                } else {
                    if widgets::ghost_button(ui, &palette, "Check now").clicked() {
                        self.check_updates_now();
                    }
                    ui.label(RichText::new(format!("v{} is current", crate::VERSION)).size(11.0).color(self.palette.faint));
                }
            });
        });
    }

    fn security_section(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Vault encryption");
            let current = self.core.protection();
            ui.label(RichText::new(format!("Current: {}", current.label())).size(12.0).color(self.palette.muted));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if current != Protection::Windows && widgets::ghost_button(ui, &palette, "Use Windows account").clicked() {
                    self.switch_protection(Protection::Windows, None);
                }
                if current != Protection::None && widgets::ghost_button(ui, &palette, "Remove encryption").clicked() {
                    self.switch_protection(Protection::None, None);
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let pw = &mut self.group_input; // reuse the scratch input field
                ui.add(egui::TextEdit::singleline(pw).password(true).hint_text("password").desired_width(160.0));
                if widgets::ghost_button(ui, &palette, "Set password").clicked() {
                    let password = self.group_input.clone();
                    if password.len() < 6 {
                        self.show_toast("Use at least 6 characters", true);
                    } else {
                        self.switch_protection(Protection::Password, Some(password.clone()));
                        self.group_input.clear();
                    }
                }
            });
        });
    }

    fn data_section(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Backup & data");
            ui.horizontal(|ui| {
                if widgets::ghost_button(ui, &palette, &format!("{} Export backup", egui_phosphor::regular::EXPORT)).clicked() {
                    self.export_backup();
                }
                if widgets::ghost_button(ui, &palette, &format!("{} Import backup", egui_phosphor::regular::DOWNLOAD_SIMPLE)).clicked() {
                    self.import_backup();
                }
                if ui.button(format!("{} Open data folder", egui_phosphor::regular::FOLDER_OPEN)).clicked() {
                    let _ = open_folder(self.core.data_dir());
                }
            });
        });
    }

    fn api_section(&mut self, ui: &mut egui::Ui, s: &mut Settings) -> bool {
        let palette = self.palette;
        let mut dirty = false;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "Local API & AI (MCP)");
            ui.label(RichText::new("A loopback server so tools like Claude and Codex can manage accounts. Token-protected; off-machine requests are refused.").size(11.0).color(self.palette.faint));
            ui.add_space(6.0);
            widgets::setting_row(ui, &palette, "Enable API server", "127.0.0.1 only", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.api_enabled)).changed();
            });
            widgets::setting_row(ui, &palette, "Port", "", |ui| {
                dirty |= ui.add(egui::DragValue::new(&mut s.api_port).range(1024..=65535)).changed();
            });
            widgets::setting_row(ui, &palette, "Expose secrets", "Allow the API to return cookies (dangerous)", |ui| {
                dirty |= ui.add(egui::Checkbox::without_text(&mut s.api_expose_secrets)).changed();
            });
            let token = self.core.secret("api_token").unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Token").size(12.0).color(self.palette.muted));
                let shown = if token.is_empty() {
                    "(none — generate one)".to_owned()
                } else {
                    format!("{}…{}", &token[..token.len().min(6)], &token[token.len().saturating_sub(4)..])
                };
                ui.label(RichText::new(shown).monospace().size(11.0).color(self.palette.text));
                if ui.button(egui_phosphor::regular::COPY).on_hover_text("Copy token").clicked() {
                    ui.ctx().copy_text(token.clone());
                    self.show_toast("Token copied", false);
                }
                if ui.button(egui_phosphor::regular::ARROWS_CLOCKWISE).on_hover_text("Regenerate").clicked() {
                    let new = crate::store::crypto::b64(&crate::store::crypto::random_bytes::<24>());
                    let _ = self.core.set_secret("api_token", &new);
                    self.show_toast("New API token generated", false);
                }
            });
            if ui.button(format!("{} API documentation", egui_phosphor::regular::BOOK_OPEN)).clicked() {
                let _ = open_url(&format!("https://github.com/{}/blob/main/docs/API.md", crate::REPO));
            }
        });
        dirty
    }

    fn about_section(&self, ui: &mut egui::Ui) {
        let palette = self.palette;
        widgets::card(ui, &palette, |ui| {
            widgets::section(ui, &palette, "About");
            ui.label(RichText::new(format!("{} v{}", crate::APP_NAME, crate::VERSION)).strong().color(self.palette.text));
            ui.label(
                RichText::new("A fast Rust rewrite, forked from Evanovar RAM, which drew on ic3w0lf's original Roblox Account Manager.")
                    .size(11.0)
                    .color(self.palette.muted),
            );
            ui.label(RichText::new("Licensed under GPL-3.0.").size(11.0).color(self.palette.faint));
        });
    }

    // ---- Actions ----

    /// Re-encrypts the vault off the UI thread (a password change runs Argon2).
    fn switch_protection(&mut self, protection: Protection, password: Option<String>) {
        self.show_toast("Changing vault encryption…", false);
        self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |core, _| {
            match core.set_protection(protection, password.as_deref()) {
                Ok(()) => Msg::Toast(format!("Vault now: {}", protection.label()), false),
                Err(err) => Msg::Toast(err.message, true),
            }
        });
    }

    fn check_updates_now(&mut self) {
        let beta = self.core.settings().update_channel == UpdateChannel::Beta;
        self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |_, services| {
            match crate::services::updater::check(beta) {
                Ok(Some(info)) => {
                    services.live.set_update_available(Some(info.version.clone()));
                    Msg::Toast(format!("Update available: v{}", info.version), false)
                }
                Ok(None) => Msg::Toast("You're on the latest version".into(), false),
                Err(err) => Msg::Toast(err.message, true),
            }
        });
    }

    fn install_update(&mut self) {
        let beta = self.core.settings().update_channel == UpdateChannel::Beta;
        self.show_toast("Downloading update…", false);
        self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), move |_, _| match crate::services::updater::check(beta) {
            Ok(Some(info)) => match crate::services::updater::download_and_apply(&info) {
                Ok(()) => Msg::Toast("Update ready — close Nova to finish installing".into(), false),
                Err(err) => Msg::Toast(err.message, true),
            },
            Ok(None) => Msg::Toast("No update to install".into(), false),
            Err(err) => Msg::Toast(err.message, true),
        });
    }

    fn export_backup(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_file_name("accounts.novabackup").add_filter("Nova backup", &["novabackup"]).save_file()
        else {
            return;
        };
        // Ask for a password via the scratch field is clumsy; use a fixed prompt modal instead.
        let password = self.group_input.clone();
        if password.len() < 8 {
            self.show_toast("Type a backup password (8+ chars) in the Vault password box first", true);
            return;
        }
        let accounts = self.core.accounts();
        match crate::import::backup::export(&accounts, &path, &password) {
            Ok(count) => self.show_toast(format!("Exported {count} account(s)"), false),
            Err(err) => self.show_toast(err.message, true),
        }
    }

    fn import_backup(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Backup", &["novabackup", "json"]).pick_file() else { return };
        let password = self.group_input.clone();
        match crate::import::backup::import(&path, &password) {
            Ok(batch) => match self.core.apply_import(&batch) {
                Ok(outcome) => {
                    self.reload_accounts();
                    self.show_toast(format!("Imported {} added, {} updated", outcome.added, outcome.updated), false);
                }
                Err(err) => self.show_toast(err.message, true),
            },
            Err(err) => self.show_toast(format!("{} (type the backup password in the Vault box)", err.message), true),
        }
    }
}

#[cfg(windows)]
fn open_folder(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("explorer").arg(path).spawn().map(|_| ())
}
#[cfg(not(windows))]
fn open_folder(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn().map(|_| ())
}
#[cfg(not(windows))]
fn open_url(_url: &str) -> std::io::Result<()> {
    Ok(())
}
