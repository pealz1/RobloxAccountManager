//! Account Activity Monitor: maps running clients to accounts and reports memory use.
//!
//! Roblox logs don't name their PID, so each client is matched to the account log
//! whose start time is closest to the process's creation time (within a window).

use super::{interruptible_sleep, Activity, Instance, Services};
use crate::roblox::logs;
use crate::win::process::{self, ProcId};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

/// FILETIME (100 ns since 1601) → UTC.
fn filetime_to_utc(filetime: u64) -> Option<DateTime<Utc>> {
    const EPOCH_DIFF_SECS: i64 = 11_644_473_600;
    let secs = (filetime / 10_000_000) as i64 - EPOCH_DIFF_SECS;
    let nanos = ((filetime % 10_000_000) * 100) as u32;
    DateTime::from_timestamp(secs, nanos)
}

/// Best-effort map of running client `(ProcId)` → account user id, using the logs.
pub fn match_processes() -> HashMap<ProcId, u64> {
    let processes = process::list_roblox();
    let mut logs = logs::scan_logs();
    logs.retain(|l| l.user_id > 0 && l.started.is_some());
    let mut used = vec![false; logs.len()];
    let mut map = HashMap::new();
    // Oldest process first so early clients claim earlier logs.
    let mut sorted = processes;
    sorted.sort_by_key(|p| p.id.created);
    for proc in sorted {
        let Some(created) = filetime_to_utc(proc.id.created) else { continue };
        let mut best: Option<(i64, usize)> = None;
        for (index, log) in logs.iter().enumerate() {
            if used[index] {
                continue;
            }
            let started = log.started.unwrap();
            let diff = (created - started).num_seconds();
            // The log is written shortly before the process stabilises.
            if (-ChronoDuration::seconds(10).num_seconds()..=90).contains(&diff) {
                let score = diff.abs();
                if best.is_none_or(|(b, _)| score < b) {
                    best = Some((score, index));
                }
            }
        }
        if let Some((_, index)) = best {
            used[index] = true;
            map.insert(proc.id, logs[index].user_id);
        }
    }
    map
}

/// Aggregates per-account activity from the current process→account map.
pub fn snapshot() -> (HashMap<u64, Activity>, HashMap<ProcId, u64>) {
    let map = match_processes();
    let mut activity: HashMap<u64, Activity> = HashMap::new();
    for (proc, user_id) in &map {
        let entry = activity.entry(*user_id).or_default();
        entry.running = true;
        entry.client_count += 1;
        entry.ram_mb += process::memory_mb(proc.pid).unwrap_or(0.0);
    }
    (activity, map)
}

pub fn start(services: &Arc<Services>) {
    let stop = Arc::new(AtomicBool::new(false));
    let body_stop = Arc::clone(&stop);
    let body_services = Arc::clone(services);
    let worker = super::Worker::spawn("activity-monitor", stop, move || run(body_services, &body_stop));
    services.add_worker(worker);
}

fn run(services: Arc<Services>, stop: &AtomicBool) {
    loop {
        let interval = Duration::from_secs(services.core.settings().instance_scan_secs.max(2) as u64);
        let (activity, map) = snapshot();
        services.live.set_activity(activity);
        services.live.set_instances(instances_from(&map));
        services.request_repaint();
        if interruptible_sleep(stop, interval) {
            return;
        }
    }
}

fn instances_from(map: &HashMap<ProcId, u64>) -> Vec<Instance> {
    let mut names: HashMap<u64, String> = HashMap::new();
    let mut list: Vec<Instance> = map
        .iter()
        .map(|(proc, user_id)| {
            let username = names.entry(*user_id).or_default().clone();
            Instance { pid: proc.pid, user_id: *user_id, username, hidden: false }
        })
        .collect();
    list.sort_by_key(|i| i.pid);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_converts_to_utc() {
        // 2007-01-01 00:00:00 UTC → (unix + 11644473600) * 1e7 FILETIME units.
        let unix = 1_167_609_600u64;
        let ft = (unix + 11_644_473_600) * 10_000_000;
        let dt = filetime_to_utc(ft).unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2007-01-01 00:00:00");
    }

    #[test]
    fn matching_does_not_panic_without_roblox() {
        let _ = match_processes();
    }
}
