//! Launch orchestration: single and batch joins, with the inter-launch delay and
//! (in handle mode) waiting for each client to clear its singleton handles.

use super::Core;
use crate::error::{AppError, AppResult};
use crate::roblox::account as api;
use crate::roblox::games;
use crate::roblox::launch::{self, LaunchRequest, Launched};
use crate::roblox::private_servers;
use crate::store::model::{CookieStatus, ServerVisit};
use chrono::Utc;
use std::time::{Duration, Instant};

/// Result of launching one or more accounts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchResult {
    pub launched: usize,
    pub total: usize,
    /// (account, error) for each failure.
    pub failures: Vec<(String, AppError)>,
}

impl BatchResult {
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
    }
    pub fn summary(&self, verb: &str) -> String {
        format!("{verb} {}/{} account(s).", self.launched, self.total)
    }
}

impl Core {
    /// Launches one account into a target.
    pub fn launch(&self, reference: &str, request: &LaunchRequest) -> AppResult<Launched> {
        let account = self.account(reference).ok_or_else(|| AppError::not_found("Account", reference))?;
        if account.cookie.is_empty() {
            return Err(AppError::new("COOKIE_MISSING", "Account Cookie Missing", format!("{} has no saved cookie.", account.label())));
        }
        if account.cookie_status == CookieStatus::Invalid {
            return Err(AppError::new(
                "COOKIE_INVALID",
                "Account Cookie Invalid",
                format!("{}'s cookie is invalid. Re-add the account.", account.label()),
            ));
        }
        let settings = self.settings();
        // Apply the basic Roblox settings presets before the client starts. Best-effort:
        // a problem here is logged but does not block the launch.
        if let Err(err) = crate::services::roblox_settings::apply_presets(&settings.roblox) {
            crate::log_warn!("Could not apply Roblox settings presets: {err}");
        }
        let launched = launch::launch(&account.cookie, request, settings.launcher, &settings.custom_launcher_path)?;
        // Launching proves the cookie worked.
        let _ = self.edit(|data| {
            if let Some(acc) = data.find_mut(reference) {
                acc.cookie_status = CookieStatus::Valid;
                acc.last_used = Some(Utc::now());
            }
        });
        if settings.track_server_history && !launched.place_id.is_empty() {
            let _ = self.record_launch_visit(&account, &launched, request);
        }
        if !launched.place_id.is_empty()
            && let Ok(place_id) = launched.place_id.parse::<u64>()
        {
            let name = games::game_name(place_id).unwrap_or_default();
            let _ = self.record_recent_game(place_id, &name, &request.private_server);
        }
        Ok(launched)
    }

    fn record_launch_visit(&self, account: &crate::store::model::Account, launched: &Launched, request: &LaunchRequest) -> AppResult<()> {
        let place_id = launched.place_id.parse().unwrap_or(0);
        let visit = ServerVisit {
            at: Some(Utc::now()),
            user_id: account.user_id,
            username: account.username.clone(),
            place_id,
            job_id: request.job_id.clone(),
            private_server: request.private_server.clone(),
            ..Default::default()
        };
        self.edit(|data| data.push_visit(visit))
    }

    /// Launches several accounts into the same target, honouring the launch delay
    /// and, in handle mode, waiting for each client to be ready before the next.
    pub fn launch_batch(&self, references: &[String], request: &LaunchRequest) -> BatchResult {
        let settings = self.settings();
        let delay = Duration::from_secs_f32(settings.launch_delay_secs.max(0.0));
        let mut result = BatchResult { total: references.len(), ..Default::default() };
        for (index, reference) in references.iter().enumerate() {
            match self.launch(reference, request) {
                Ok(launched) => {
                    result.launched += 1;
                    self.wait_ready_in_handle_mode(launched.browser_tracker_id);
                }
                Err(err) => result.failures.push((reference.clone(), err)),
            }
            if index + 1 < references.len() && delay > Duration::ZERO {
                std::thread::sleep(delay);
            }
        }
        result
    }

    /// In handle mode, give the new client time to register and have its singleton
    /// handles cleared, so the next launch is not blocked.
    fn wait_ready_in_handle_mode(&self, _tracker: u64) {
        let Some(guard) = self.multi_guard() else { return };
        if guard.mode() != crate::store::settings::MultiMethod::Handle {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let ids: Vec<_> = crate::win::process::list_roblox().into_iter().map(|p| p.id).collect();
            if !ids.is_empty() && guard.handles_ready(&ids) {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Joins the game a target user is currently in (visible to the first account).
    pub fn join_user(&self, references: &[String], target: &str) -> AppResult<BatchResult> {
        let first = references
            .first()
            .and_then(|r| self.account(r))
            .ok_or_else(|| AppError::invalid("NO_ACCOUNT", "Select an account to join with."))?;
        let target_id = if target.chars().all(|c| c.is_ascii_digit()) {
            target.parse().map_err(|_| AppError::invalid("TARGET_INVALID", "Enter a username or user id."))?
        } else {
            api::user_id_from_username(target)?
        };
        let presence = api::presence(target_id, &first.cookie)?;
        if !presence.in_game {
            return Err(AppError::new("TARGET_NOT_IN_GAME", "User Not In A Game", format!("{target} is not in a game right now.")));
        }
        let place_id = presence.place_id.or(presence.root_place_id).ok_or_else(|| {
            AppError::new(
                "TARGET_PLACE_HIDDEN",
                "Join Not Allowed",
                format!("{target}'s game cannot be joined (their join setting is restricted)."),
            )
        })?;
        let request = LaunchRequest { place_id: place_id.to_string(), job_id: presence.job_id, ..Default::default() };
        Ok(self.launch_batch(references, &request))
    }

    /// Joins the public server with the fewest players.
    pub fn join_small_server(&self, references: &[String], place_id: u64) -> AppResult<BatchResult> {
        let job_id = games::smallest_server(place_id)?;
        let request = LaunchRequest { place_id: place_id.to_string(), job_id, ..Default::default() };
        Ok(self.launch_batch(references, &request))
    }

    /// Resolves a share/VIP link and launches into the private server.
    pub fn join_private_server(&self, references: &[String], link: &str) -> AppResult<BatchResult> {
        let cookie = references.first().and_then(|r| self.account(r)).map(|a| a.cookie);
        let (place, code) = private_servers::resolve_share_link(link, cookie.as_deref())?;
        if code.is_empty() {
            return Err(AppError::invalid("PRIVATE_SERVER_INVALID", "That private server link could not be read."));
        }
        let request = LaunchRequest {
            place_id: place.unwrap_or_default(),
            private_server: if link.chars().all(|c| c.is_ascii_digit()) { link.to_owned() } else { code },
            ..Default::default()
        };
        Ok(self.launch_batch(references, &request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_core;

    #[test]
    fn launching_unknown_account_errors() {
        let (_dir, core) = test_core();
        assert_eq!(core.launch("ghost", &LaunchRequest::default()).err().unwrap().code, "NOT_FOUND");
    }

    #[test]
    fn batch_summary_counts() {
        let r = BatchResult { launched: 2, total: 3, failures: vec![("x".into(), AppError::not_found("Account", "x"))] };
        assert_eq!(r.summary("Joined"), "Joined 2/3 account(s).");
        assert!(!r.ok());
    }
}
