//! The desktop GUI (egui/eframe).

mod task;
mod theme;
mod unlock;
mod widgets;
mod pages {
    pub mod accounts;
    pub mod add_account;
    pub mod anti_afk;
    pub mod multi;
    pub mod private_servers;
    pub mod rejoin;
    pub mod servers;
    pub mod settings;
}

use crate::core::Core;
use crate::services::{Services, auto_rejoin::RejoinManager};
use crate::store::model::Account;
use eframe::egui::{self, Color32, RichText};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use task::{Msg, TaskSender, Tasks};
use theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Accounts,
    Servers,
    PrivateServers,
    AutoRejoin,
    AntiAfk,
    MultiRoblox,
    Settings,
}

impl Page {
    const NAV: [(Page, &'static str, &'static str); 7] = [
        (Page::Accounts, egui_phosphor::regular::USERS_THREE, "Accounts"),
        (Page::Servers, egui_phosphor::regular::GLOBE_HEMISPHERE_WEST, "Servers"),
        (Page::PrivateServers, egui_phosphor::regular::LOCK_KEY, "Private Servers"),
        (Page::AutoRejoin, egui_phosphor::regular::ARROWS_CLOCKWISE, "Auto-Rejoin"),
        (Page::AntiAfk, egui_phosphor::regular::PERSON_SIMPLE_WALK, "Anti-AFK"),
        (Page::MultiRoblox, egui_phosphor::regular::STACK, "Multi Roblox"),
        (Page::Settings, egui_phosphor::regular::GEAR_SIX, "Settings"),
    ];
}

/// State for the launch bar shared by the Accounts page.
#[derive(Default)]
pub struct LaunchBar {
    pub place_id: String,
    pub private_server: String,
    pub job_id: String,
    pub game_name: String,
    pub join_user: String,
}

pub struct NovaApp {
    pub core: Arc<Core>,
    pub services: Arc<Services>,
    pub tasks: Tasks,
    pub sender: TaskSender,
    pub palette: Palette,
    pub system_dark: bool,
    pub page: Page,

    pub accounts: Vec<Account>,
    pub revision: u64,
    pub selection: HashSet<String>,
    pub last_clicked: Option<String>,
    pub search: String,
    pub group_filter: Option<String>,

    pub launch: LaunchBar,
    pub rejoin: Arc<RejoinManager>,
    pub anti_afk: Option<crate::services::anti_afk::AntiAfkHandle>,

    pub avatars: HashMap<u64, egui::TextureHandle>,
    pub avatar_pending: HashSet<u64>,
    pub game_names: HashMap<u64, String>,
    pub game_name_pending: HashSet<u64>,

    pub toast: Option<(String, bool, Instant)>,
    pub add_dialog: Option<pages::add_account::AddDialog>,
    pub rejoin_dialog: Option<pages::rejoin::RejoinDialog>,
    pub ps_state: pages::private_servers::PrivateServersState,
    pub confirm: Option<Confirm>,
    pub group_input: String,
    pub renaming_group: Option<(String, String)>,
}

/// A pending confirmation modal.
pub struct Confirm {
    pub title: String,
    pub body: String,
    pub action: Box<dyn FnOnce(&mut NovaApp) + Send>,
}

impl NovaApp {
    fn new(cc: &eframe::CreationContext<'_>, core: Arc<Core>, services: Arc<Services>) -> NovaApp {
        theme::install_fonts(&cc.egui_ctx);
        let system_dark = !matches!(cc.egui_ctx.theme(), egui::Theme::Light);
        let settings = core.settings();
        let palette = theme::palette(settings.theme, &settings.accent, system_dark);
        theme::apply(&cc.egui_ctx, &palette, settings.ui_scale);

        let tasks = Tasks::new(cc.egui_ctx.clone());
        let sender = tasks.sender();
        let ctx = cc.egui_ctx.clone();
        services.set_repaint(move || ctx.request_repaint());

        let mut app = NovaApp {
            core: Arc::clone(&core),
            services,
            tasks,
            sender,
            palette,
            system_dark,
            page: Page::Accounts,
            accounts: Vec::new(),
            revision: 0,
            selection: HashSet::new(),
            last_clicked: None,
            search: String::new(),
            group_filter: None,
            launch: LaunchBar::default(),
            rejoin: Arc::new(RejoinManager::default()),
            anti_afk: None,
            avatars: HashMap::new(),
            avatar_pending: HashSet::new(),
            game_names: HashMap::new(),
            game_name_pending: HashSet::new(),
            toast: None,
            add_dialog: None,
            rejoin_dialog: None,
            ps_state: Default::default(),
            confirm: None,
            group_input: String::new(),
            renaming_group: None,
        };
        app.reload_accounts();
        app
    }

    pub fn reload_accounts(&mut self) {
        self.accounts = self.core.accounts();
        self.revision = self.core.revision();
        let keys: HashSet<String> = self.accounts.iter().map(Account::key).collect();
        self.selection.retain(|k| keys.contains(k));
    }

    /// Accounts after the search and group filters are applied.
    pub fn visible_accounts(&self) -> Vec<Account> {
        let terms: Vec<String> = self.search.to_lowercase().split_whitespace().map(str::to_owned).collect();
        self.accounts
            .iter()
            .filter(|a| self.group_filter.as_ref().is_none_or(|g| &a.group == g))
            .filter(|a| {
                if terms.is_empty() {
                    return true;
                }
                let haystack = format!("{} {} {} {} {}", a.username, a.alias, a.note, a.group, a.user_id).to_lowercase();
                terms.iter().all(|t| haystack.contains(t))
            })
            .cloned()
            .collect()
    }

    pub fn selected_keys(&self) -> Vec<String> {
        self.accounts.iter().map(|a| a.key()).filter(|k| self.selection.contains(k)).collect()
    }

    pub fn show_toast(&mut self, message: impl Into<String>, error: bool) {
        self.toast = Some((message.into(), error, Instant::now()));
    }

    /// Returns a texture for an account avatar, fetching it in the background if missing.
    pub fn avatar_texture(&mut self, ctx: &egui::Context, user_id: u64) -> Option<egui::TextureHandle> {
        if user_id == 0 {
            return None;
        }
        if let Some(handle) = self.avatars.get(&user_id) {
            return Some(handle.clone());
        }
        if let Some(bytes) = self.services.avatars.cached(user_id)
            && let Ok(image) = image::load_from_memory(&bytes)
        {
            let rgba = image.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
            let handle = ctx.load_texture(format!("avatar-{user_id}"), color, egui::TextureOptions::LINEAR);
            self.avatars.insert(user_id, handle.clone());
            self.avatar_pending.remove(&user_id);
            return Some(handle);
        }
        if self.avatar_pending.insert(user_id) && self.services.avatars.needs_refresh(user_id) {
            let avatars = Arc::clone(&self.services.avatars);
            let sender = self.sender.clone();
            std::thread::Builder::new()
                .name("avatar-fetch".into())
                .spawn(move || {
                    avatars.refresh(&[user_id]);
                    sender.post(Msg::Run(Box::new(move |_app| {})));
                })
                .ok();
        }
        None
    }

    fn drain_messages(&mut self, ctx: &egui::Context) {
        for msg in self.tasks.drain() {
            match msg {
                Msg::Toast(text, error) => self.show_toast(text, error),
                Msg::AccountsChanged => self.reload_accounts(),
                Msg::GameName(place, name) => {
                    self.game_name_pending.remove(&place);
                    if self.launch.place_id.trim() == place.to_string() {
                        self.launch.game_name = name.clone();
                    }
                    self.game_names.insert(place, name);
                }
                Msg::PrivateServers(result) => self.ps_state.on_loaded(result),
                Msg::QuickLogin(q) => pages::add_account::on_quick_login(self, q),
                Msg::Run(callback) => callback(self),
            }
        }
        // Background workers also raise toasts through Live.
        if let Some((text, error)) = self.services.live.take_toast() {
            self.show_toast(text, error);
        }
        let _ = ctx;
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new(egui_phosphor::regular::ROCKET_LAUNCH).size(22.0).color(self.palette.accent));
            ui.label(RichText::new("Nova RAM").size(18.0).strong().color(self.palette.text));
        });
        ui.add_space(16.0);

        for (page, icon, label) in Page::NAV {
            let selected = self.page == page;
            let (text_color, bg) =
                if selected { (self.palette.accent_text, self.palette.accent) } else { (self.palette.muted, Color32::TRANSPARENT) };
            let label_text = format!("{icon}  {label}");
            let button = egui::Button::new(RichText::new(label_text).color(text_color).size(13.5))
                .fill(bg)
                .corner_radius(9.0)
                .min_size(egui::vec2(ui.available_width() - 16.0, 34.0));
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                if ui.add(button).clicked() {
                    self.page = page;
                }
            });
            ui.add_space(2.0);
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            ui.label(RichText::new(format!("v{}", crate::VERSION)).size(10.0).color(self.palette.faint));
            if let Some(version) = self.services.live.update_available()
                && ui
                    .button(
                        RichText::new(format!("{} Update to v{version}", egui_phosphor::regular::DOWNLOAD_SIMPLE))
                            .color(self.palette.accent),
                    )
                    .clicked()
            {
                self.page = Page::Settings;
            }
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("{}  Kill all Roblox", egui_phosphor::regular::X_CIRCLE)).color(self.palette.danger),
                    )
                    .fill(Color32::TRANSPARENT),
                )
                .clicked()
            {
                // kill_all waits up to a few seconds per process, so run it off the UI thread.
                self.sender.spawn(Arc::clone(&self.core), Arc::clone(&self.services), |_, _| {
                    let (closed, remaining) = crate::win::process::kill_all();
                    Msg::Toast(format!("Closed {closed} client(s), {remaining} remaining"), remaining > 0)
                });
            }
        });
    }

    fn render_page(&mut self, ui: &mut egui::Ui) {
        match self.page {
            Page::Accounts => self.page_accounts(ui),
            Page::Servers => self.page_servers(ui),
            Page::PrivateServers => self.page_private_servers(ui),
            Page::AutoRejoin => self.page_auto_rejoin(ui),
            Page::AntiAfk => self.page_anti_afk(ui),
            Page::MultiRoblox => self.page_multi(ui),
            Page::Settings => self.page_settings(ui),
        }
    }

    fn overlay(&mut self, ctx: &egui::Context) {
        if let Some(dialog) = self.add_dialog.take() {
            self.add_dialog = pages::add_account::show(self, ctx, dialog);
        }
        if let Some(dialog) = self.rejoin_dialog.take() {
            self.rejoin_dialog = pages::rejoin::show_dialog(self, ctx, dialog);
        }
        if let Some(confirm) = self.confirm.take() {
            let mut keep = true;
            let mut run = false;
            egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new(&confirm.title).size(16.0).strong());
                ui.add_space(6.0);
                ui.label(&confirm.body);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if widgets::ghost_button(ui, &self.palette, "Cancel").clicked() {
                        keep = false;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Confirm").color(Color32::WHITE))
                                    .fill(self.palette.danger)
                                    .corner_radius(8.0),
                            )
                            .clicked()
                        {
                            run = true;
                            keep = false;
                        }
                    });
                });
            });
            if run {
                (confirm.action)(self);
            } else if keep {
                self.confirm = Some(confirm);
            }
        }
    }

    fn toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((text, error, shown)) = self.toast.clone() else { return };
        if shown.elapsed().as_secs_f32() > 4.0 {
            self.toast = None;
            return;
        }
        let color = if error { self.palette.danger } else { self.palette.online };
        egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0)).show(ctx, |ui| {
            egui::Frame::new()
                .fill(self.palette.elevated)
                .stroke(egui::Stroke::new(1.0, color))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    let icon = if error { egui_phosphor::regular::WARNING_CIRCLE } else { egui_phosphor::regular::CHECK_CIRCLE };
                    ui.label(RichText::new(format!("{icon}  {text}")).color(self.palette.text));
                });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl eframe::App for NovaApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain_messages(&ctx);
        if self.core.revision() != self.revision {
            self.reload_accounts();
        }

        egui::Panel::left("nav")
            .exact_size(190.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(self.palette.surface))
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.palette.bg).inner_margin(egui::Margin::same(16)))
            .show(ui, |ui| self.render_page(ui));

        self.overlay(&ctx);
        self.toast_overlay(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.rejoin.stop_all();
        self.services.stop_all();
    }
}

/// Launches the desktop app. Blocks until the window closes.
pub fn run(core: Arc<Core>, services: Arc<Services>) -> eframe::Result<()> {
    let icon = load_icon();
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1060.0, 680.0])
        .with_min_inner_size([820.0, 540.0])
        .with_title(format!("{} v{}", crate::APP_NAME, crate::VERSION))
        .with_icon(icon);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Nova RAM",
        options,
        Box::new(move |cc| {
            let app = NovaApp::new(cc, Arc::clone(&core), Arc::clone(&services));
            services.start_background();
            Ok(Box::new(app))
        }),
    )
}

fn load_icon() -> egui::IconData {
    // Fall back to an empty icon if the bundled asset is missing.
    let bytes = include_bytes!("../../assets/icon.ico");
    match image::load_from_memory(bytes) {
        Ok(image) => {
            let rgba = image.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            egui::IconData { rgba: rgba.into_raw(), width: w, height: h }
        }
        Err(_) => egui::IconData { rgba: vec![0; 4], width: 1, height: 1 },
    }
}

/// Shows the vault unlock window; returns the opened core or None if the user quit.
pub fn unlock_prompt(data_dir: &std::path::Path) -> Option<Arc<Core>> {
    unlock::prompt(data_dir)
}

/// Shows a blocking native error dialog for a fatal startup problem.
#[cfg(windows)]
pub fn fatal_dialog(message: &str) {
    use std::os::windows::ffi::OsStrExt;
    let text: Vec<u16> = std::ffi::OsStr::new(message).encode_wide().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = std::ffi::OsStr::new("Nova RAM").encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are null-terminated; MessageBoxW only reads them.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
    }
}

#[cfg(not(windows))]
pub fn fatal_dialog(message: &str) {
    eprintln!("{message}");
}
