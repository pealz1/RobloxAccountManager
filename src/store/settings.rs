//! Non-secret preferences in `settings.json` (port of `settings_store.py`).
//! Unknown or missing fields fall back to defaults, so old files keep loading.

use super::atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Launcher {
    /// Let Windows open the `roblox-player:` link with whatever is registered.
    #[default]
    Default,
    /// Start the newest installed RobloxPlayerBeta.exe directly.
    Client,
    Bloxstrap,
    Fishstrap,
    Froststrap,
    Voidstrap,
    Custom,
}

impl Launcher {
    pub const ALL: [Launcher; 7] = [
        Launcher::Default,
        Launcher::Client,
        Launcher::Bloxstrap,
        Launcher::Fishstrap,
        Launcher::Froststrap,
        Launcher::Voidstrap,
        Launcher::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Launcher::Default => "Automatic (Windows default)",
            Launcher::Client => "Roblox Player (direct)",
            Launcher::Bloxstrap => "Bloxstrap",
            Launcher::Fishstrap => "Fishstrap",
            Launcher::Froststrap => "Froststrap",
            Launcher::Voidstrap => "Voidstrap",
            Launcher::Custom => "Custom executable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiMethod {
    /// Hold Roblox's singleton mutex/event before clients start (Roblox must be closed first).
    #[default]
    Mutex,
    /// Close the singleton handles inside each new client (works with clients already open).
    Handle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowTitleMode {
    #[default]
    Username,
    Alias,
    Note,
    UsernameAndNote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AntiAfk {
    pub enabled: bool,
    /// Key name ("w", "space", "f5"), or mouse action ("lmb", "rmb", "mmb", "mback", "mfwd", "scroll_up", "scroll_down").
    pub action: String,
    pub press_count: u32,
    pub interval_minutes: u32,
    pub show_countdown: bool,
}

impl Default for AntiAfk {
    fn default() -> Self {
        Self { enabled: false, action: "w".into(), press_count: 1, interval_minutes: 10, show_countdown: true }
    }
}

/// Basic Roblox settings applied before launch (port of the "Basic presets").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RobloxPresets {
    pub framerate_cap_enabled: bool,
    pub framerate_cap: u32,
    pub master_volume_enabled: bool,
    pub master_volume: f32,
    pub graphics_quality_enabled: bool,
    pub graphics_quality: u32,
    pub advanced_auto_apply: bool,
    /// Custom path of GlobalBasicSettings_13.xml, empty = default location.
    pub settings_path: String,
}

impl Default for RobloxPresets {
    fn default() -> Self {
        Self {
            framerate_cap_enabled: false,
            framerate_cap: 60,
            master_volume_enabled: false,
            master_volume: 0.5,
            graphics_quality_enabled: false,
            graphics_quality: 1,
            advanced_auto_apply: false,
            settings_path: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Appearance
    pub theme: ThemeMode,
    pub accent: String,
    pub ui_scale: f32,
    pub compact_rows: bool,
    pub show_avatars: bool,

    // Window and app
    pub always_on_top: bool,
    pub close_to_tray: bool,
    pub start_with_windows: bool,
    pub start_menu_shortcut: bool,
    pub start_minimized: bool,
    pub log_retention: usize,

    // Updates
    pub check_updates: bool,
    pub update_channel: UpdateChannel,
    pub skipped_version: String,

    // Launching
    pub launcher: Launcher,
    pub custom_launcher_path: String,
    pub confirm_before_launch: bool,
    pub launch_delay_secs: f32,
    pub max_recent_games: usize,
    pub last_place_id: String,
    pub last_private_server: String,
    pub last_job_id: String,

    // Multi Roblox and windows
    pub multi_roblox: bool,
    pub multi_method: MultiMethod,
    pub cookie_lock_773: bool,
    pub rename_windows: bool,
    pub window_title_mode: WindowTitleMode,
    pub window_grid_hotkey_enabled: bool,
    pub window_grid_hotkey: String,
    pub installer_fix: bool,
    pub optimize_ram: bool,
    pub ram_limit_mb: u32,
    pub activity_monitor: bool,
    pub instance_scan_secs: u32,
    pub track_server_history: bool,

    // Accounts
    pub validate_cookies_on_startup: bool,
    pub cookie_validation_delay_secs: f32,
    pub avatar_cache_days: u32,
    pub allow_copy_secrets: bool,
    pub refresh_account_stats: bool,

    // Browser sign-in
    pub browser: String,
    pub browser_path: String,
    pub browser_batch_size: u32,

    pub anti_afk: AntiAfk,
    pub roblox: RobloxPresets,

    // Roblox downloader
    pub downloader_folder: String,
    pub downloader_version: String,
    pub downloader_customizations: bool,

    // Local API / MCP
    pub api_enabled: bool,
    pub api_port: u16,
    pub api_expose_secrets: bool,
    pub api_allowed_origins: Vec<String>,
    pub api_max_auth_failures: u32,

    // Auto-Rejoin
    pub connectivity_check_urls: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            accent: "#5B8CFF".into(),
            ui_scale: 1.0,
            compact_rows: false,
            show_avatars: true,
            always_on_top: false,
            close_to_tray: false,
            start_with_windows: false,
            start_menu_shortcut: false,
            start_minimized: false,
            log_retention: crate::logging::DEFAULT_RETENTION,
            check_updates: true,
            update_channel: UpdateChannel::Stable,
            skipped_version: String::new(),
            launcher: Launcher::Default,
            custom_launcher_path: String::new(),
            confirm_before_launch: false,
            launch_delay_secs: 0.5,
            max_recent_games: 10,
            last_place_id: String::new(),
            last_private_server: String::new(),
            last_job_id: String::new(),
            multi_roblox: true,
            multi_method: MultiMethod::Mutex,
            cookie_lock_773: true,
            rename_windows: false,
            window_title_mode: WindowTitleMode::Username,
            window_grid_hotkey_enabled: false,
            window_grid_hotkey: "Ctrl+Shift+A".into(),
            installer_fix: false,
            optimize_ram: false,
            ram_limit_mb: 750,
            activity_monitor: true,
            instance_scan_secs: 5,
            track_server_history: true,
            validate_cookies_on_startup: true,
            cookie_validation_delay_secs: 1.5,
            avatar_cache_days: 7,
            allow_copy_secrets: false,
            refresh_account_stats: true,
            browser: "auto".into(),
            browser_path: String::new(),
            browser_batch_size: 5,
            anti_afk: AntiAfk::default(),
            roblox: RobloxPresets::default(),
            downloader_folder: String::new(),
            downloader_version: "LIVE".into(),
            downloader_customizations: false,
            api_enabled: true,
            api_port: 7963,
            api_expose_secrets: false,
            api_allowed_origins: Vec::new(),
            api_max_auth_failures: 10,
            connectivity_check_urls: vec!["https://www.google.com/generate_204".into(), "https://www.cloudflare.com/cdn-cgi/trace".into()],
        }
    }
}

impl Settings {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &Path) -> Settings {
        let path = Self::path_in(dir);
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(settings) => settings.sanitized(),
                Err(err) => {
                    crate::log_warn!("settings.json is invalid ({err}); using defaults");
                    atomic::quarantine(&path);
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        atomic::write_json(&Self::path_in(dir), self)
    }

    /// Clamps values that came from a hand-edited file.
    pub fn sanitized(mut self) -> Settings {
        self.ui_scale = clamp_f(self.ui_scale, 0.75, 2.0, 1.0);
        self.launch_delay_secs = clamp_f(self.launch_delay_secs, 0.0, 300.0, 0.5);
        self.cookie_validation_delay_secs = clamp_f(self.cookie_validation_delay_secs, 0.5, 60.0, 1.5);
        self.browser_batch_size = self.browser_batch_size.clamp(1, 5);
        self.instance_scan_secs = self.instance_scan_secs.clamp(2, 60);
        self.max_recent_games = self.max_recent_games.clamp(1, 50);
        self.ram_limit_mb = self.ram_limit_mb.clamp(100, 16_384);
        self.anti_afk.press_count = self.anti_afk.press_count.clamp(1, 20);
        self.anti_afk.interval_minutes = self.anti_afk.interval_minutes.clamp(1, 19);
        self.log_retention = self.log_retention.clamp(1, 500);
        if self.api_port < 1024 {
            self.api_port = 7963;
        }
        self
    }
}

fn clamp_f(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() { value.clamp(min, max) } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults_and_values_are_clamped() {
        let parsed: Settings = serde_json::from_str(r#"{"launch_delay_secs": 9999, "api_port": 5}"#).unwrap();
        let s = parsed.sanitized();
        assert_eq!(s.launch_delay_secs, 300.0);
        assert_eq!(s.api_port, 7963);
        assert_eq!(s.window_grid_hotkey, "Ctrl+Shift+A");
    }

    #[test]
    fn save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings { accent: "#FF0000".into(), ..Settings::default() };
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).accent, "#FF0000");
    }
}
