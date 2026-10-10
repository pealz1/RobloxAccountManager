//! Collects server-join history from the Roblox client logs into the vault.
//!
//! This powers the Servers page. Joins are read from `*_last.log`, attributed to
//! the account whose log it is, and appended to the vault (deduplicated).

use crate::core::Core;
use crate::roblox::logs;
use crate::store::model::ServerVisit;
use std::collections::HashMap;
use std::sync::Arc;

/// Reads all current logs and returns the visits found, newest last, with the
/// account each log belongs to resolved from the vault when possible.
pub fn collect_visits(core: &Core) -> Vec<ServerVisit> {
    let accounts = core.snapshot();
    let by_user: HashMap<u64, &crate::store::model::Account> = accounts.accounts.iter().map(|a| (a.user_id, a)).collect();
    let mut visits = Vec::new();
    for log in logs::scan_logs() {
        if log.user_id == 0 {
            continue;
        }
        let username = by_user.get(&log.user_id).map(|a| a.username.clone()).unwrap_or_default();
        for join in logs::parse_joins(&log.path) {
            visits.push(ServerVisit {
                at: join.at.or(log.started),
                user_id: log.user_id,
                username: username.clone(),
                place_id: join.place_id,
                universe_id: join.universe_id,
                job_id: join.job_id,
                server_ip: join.server_ip,
                ..Default::default()
            });
        }
    }
    visits.sort_by_key(|v| v.at);
    visits
}

/// Reads the logs once and merges any new joins into the vault.
pub fn sync(core: &Core) {
    if !core.settings().track_server_history {
        return;
    }
    let visits = collect_visits(core);
    if visits.is_empty() {
        return;
    }
    let _ = core.edit(|data| {
        for visit in visits {
            data.push_visit(visit);
        }
    });
}

pub fn start(services: &Arc<super::Services>) {
    use std::sync::atomic::AtomicBool;
    let stop = Arc::new(AtomicBool::new(false));
    let body_stop = Arc::clone(&stop);
    let core = Arc::clone(&services.core);
    let worker_services = Arc::clone(services);
    let worker = super::Worker::spawn("history-collector", stop, move || loop {
        sync(&core);
        worker_services.request_repaint();
        if super::interruptible_sleep(&body_stop, std::time::Duration::from_secs(15)) {
            return;
        }
    });
    services.add_worker(worker);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_core;

    #[test]
    fn collect_does_not_panic() {
        let (_dir, core) = test_core();
        let _ = collect_visits(&core);
    }
}
