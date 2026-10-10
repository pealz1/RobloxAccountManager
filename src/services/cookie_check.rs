//! Validates saved cookies, updating each account's status and (when valid) its
//! identity. Runs on startup and on demand, spaced out to respect rate limits.

use super::{Services, interruptible_sleep};
use crate::core::Core;
use crate::roblox::account as api;
use crate::store::model::CookieStatus;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Checks one account's cookie and records the result. Returns the new status.
pub fn check_one(core: &Core, reference: &str) -> CookieStatus {
    let Some(account) = core.account(reference) else { return CookieStatus::Unknown };
    if account.cookie.is_empty() {
        let _ = core.set_cookie_status(reference, CookieStatus::Invalid);
        return CookieStatus::Invalid;
    }
    // The cookie we are about to validate. The account may be re-logged with a new
    // cookie while this network call is in flight, so every write below only applies
    // when the stored cookie still matches this one — never stamping a verdict about
    // the old cookie onto a freshly added one.
    let checked = account.cookie.clone();
    match api::whoami(&checked) {
        Ok(identity) => {
            let _ = core.edit(|data| {
                if let Some(acc) = data.find_mut(reference) {
                    if acc.cookie != checked {
                        return; // superseded by a newer cookie
                    }
                    acc.cookie_status = CookieStatus::Valid;
                    acc.cookie_checked_at = Some(chrono::Utc::now());
                    if identity.user_id > 0 {
                        acc.user_id = identity.user_id;
                        acc.username = identity.username.clone();
                        acc.display_name = identity.display_name.clone();
                    }
                }
            });
            CookieStatus::Valid
        }
        Err(err) if err.code == "COOKIE_INVALID" => {
            let _ = core.edit(|data| {
                if let Some(acc) = data.find_mut(reference)
                    && acc.cookie == checked
                {
                    acc.cookie_status = CookieStatus::Invalid;
                    acc.cookie_checked_at = Some(chrono::Utc::now());
                }
            });
            CookieStatus::Invalid
        }
        // Network/rate-limit errors leave the status unchanged so a blip doesn't flag an account.
        Err(_) => account.cookie_status,
    }
}

/// Validates every saved cookie in the background, spaced by the configured delay.
pub fn start_all(services: &Arc<Services>) {
    let stop = Arc::new(AtomicBool::new(false));
    let body_stop = Arc::clone(&stop);
    let core = Arc::clone(&services.core);
    let worker_services = Arc::clone(services);
    let worker = super::Worker::spawn("cookie-check", stop, move || {
        let delay = Duration::from_secs_f32(core.settings().cookie_validation_delay_secs.max(0.5));
        for account in core.accounts() {
            if body_stop.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            check_one(&core, &account.key());
            worker_services.request_repaint();
            if interruptible_sleep(&body_stop, delay) {
                return;
            }
        }
    });
    services.add_worker(worker);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_core;

    #[test]
    fn empty_cookie_marks_invalid() {
        let (_dir, core) = test_core();
        core.edit(|d| {
            d.upsert(crate::store::model::Account { user_id: 1, username: "a".into(), ..Default::default() });
        })
        .unwrap();
        assert_eq!(check_one(&core, "1"), CookieStatus::Invalid);
    }
}
