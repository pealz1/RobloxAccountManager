//! Renames Roblox windows to the account they belong to, so they are easy to tell
//! apart on the taskbar and in Window Grid.

use super::{Services, interruptible_sleep};
use crate::store::settings::WindowTitleMode;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn title_for(account: &crate::store::model::Account, mode: WindowTitleMode) -> String {
    match mode {
        WindowTitleMode::Username => account.username.clone(),
        WindowTitleMode::Alias => account.label().to_owned(),
        WindowTitleMode::Note => {
            if account.note.trim().is_empty() {
                account.username.clone()
            } else {
                account.note.clone()
            }
        }
        WindowTitleMode::UsernameAndNote => {
            if account.note.trim().is_empty() {
                account.username.clone()
            } else {
                format!("{} — {}", account.username, account.note)
            }
        }
    }
}

pub fn start(services: &Arc<Services>) {
    let stop = Arc::new(AtomicBool::new(false));
    let body_stop = Arc::clone(&stop);
    let core = Arc::clone(&services.core);
    let worker = super::Worker::spawn("window-renamer", stop, move || {
        loop {
            let mode = core.settings().window_title_mode;
            let names: HashMap<u64, crate::store::model::Account> = core.accounts().into_iter().map(|a| (a.user_id, a)).collect();
            for (proc, user_id) in super::activity::match_processes() {
                if let Some(account) = names.get(&user_id) {
                    let desired = title_for(account, mode);
                    if !desired.is_empty()
                        && let Some(window) = crate::win::window::main_window(proc.pid)
                        && crate::win::window::title(window.hwnd) != desired
                    {
                        crate::win::window::set_title(window.hwnd, &desired);
                    }
                }
            }
            if interruptible_sleep(&body_stop, Duration::from_secs(3)) {
                return;
            }
        }
    });
    services.add_worker(worker);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::model::Account;

    #[test]
    fn title_modes() {
        let a = Account { username: "bob".into(), alias: "Mule".into(), note: "main".into(), ..Default::default() };
        assert_eq!(title_for(&a, WindowTitleMode::Username), "bob");
        assert_eq!(title_for(&a, WindowTitleMode::Alias), "Mule");
        assert_eq!(title_for(&a, WindowTitleMode::Note), "main");
        assert_eq!(title_for(&a, WindowTitleMode::UsernameAndNote), "bob — main");
    }
}
