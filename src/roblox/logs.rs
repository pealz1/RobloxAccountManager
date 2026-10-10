//! Parses the Roblox client logs under `%LOCALAPPDATA%\Roblox\logs`.
//!
//! Two things are read:
//! * `userid:<n>` and `browsertrackerid:<n>`, to map a running client to an account.
//! * `Joining game '<job>' place <id> at <ip>` and the following
//!   `game_join_loadtime: ... universeid:<n>`, to record which servers were joined.

use chrono::{DateTime, NaiveDateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

static TRACKER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)browsertrackerid[^0-9]{0,32}(\d+)").unwrap());
static JOIN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Joining game '([0-9a-fA-F-]+)' place (\d+) at ([0-9.]+)").unwrap()
});
static LOADTIME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"placeid:(\d+),.*?universeid:(\d+)").unwrap());
static LINE_TIME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)").unwrap());
static FILE_TIME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{8}T\d{6}Z)").unwrap());

#[derive(Debug, Clone, Default)]
pub struct LogInfo {
    pub path: PathBuf,
    pub started: Option<DateTime<Utc>>,
    pub user_id: u64,
    pub browser_tracker_id: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JoinEvent {
    pub at: Option<DateTime<Utc>>,
    pub job_id: String,
    pub place_id: u64,
    pub universe_id: u64,
    pub server_ip: String,
}

pub fn logs_dir() -> Option<PathBuf> {
    crate::paths::local_appdata().map(|p| p.join("Roblox").join("logs"))
}

/// Lists `*_last.log` files with their timestamps and (if present) user id and tracker id.
pub fn scan_logs() -> Vec<LogInfo> {
    let Some(dir) = logs_dir() else { return Vec::new() };
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with("_last.log") {
            continue;
        }
        let started = FILE_TIME_RE
            .captures(&name)
            .and_then(|c| NaiveDateTime::parse_from_str(&c[1], "%Y%m%dT%H%M%SZ").ok())
            .map(|n| n.and_utc());
        let mut info = LogInfo { path: entry.path(), started, ..Default::default() };
        if let Ok(content) = read_head(&entry.path(), 60_000) {
            let lower = content.to_lowercase();
            if let Some(uid) = lower.split("userid:").nth(1).and_then(|rest| rest.split(',').next()).and_then(|s| s.trim().parse().ok()) {
                info.user_id = uid;
            }
            if let Some(m) = TRACKER_RE.captures(&content) {
                info.browser_tracker_id = m[1].parse().unwrap_or(0);
            }
        }
        out.push(info);
    }
    out.sort_by_key(|l| l.started);
    out
}

/// Reads every server join from a single log file, in order.
pub fn parse_joins(path: &std::path::Path) -> Vec<JoinEvent> {
    let Ok(content) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut joins = Vec::new();
    let mut pending: Option<JoinEvent> = None;
    for line in content.lines() {
        if let Some(m) = JOIN_RE.captures(line) {
            if let Some(event) = pending.take() {
                joins.push(event);
            }
            pending = Some(JoinEvent {
                at: line_time(line),
                job_id: m[1].to_owned(),
                place_id: m[2].parse().unwrap_or(0),
                server_ip: m[3].to_owned(),
                universe_id: 0,
            });
        } else if let (Some(event), Some(m)) = (pending.as_mut(), LOADTIME_RE.captures(line)) {
            if event.place_id == 0 {
                event.place_id = m[1].parse().unwrap_or(0);
            }
            event.universe_id = m[2].parse().unwrap_or(0);
        }
    }
    if let Some(event) = pending.take() {
        joins.push(event);
    }
    joins
}

/// Most recent user id → log timestamp, for attributing processes started around that time.
pub fn user_id_log_times() -> HashMap<u64, DateTime<Utc>> {
    let mut map = HashMap::new();
    for info in scan_logs() {
        if info.user_id > 0 {
            if let Some(started) = info.started {
                map.entry(info.user_id).and_modify(|t| { if started > *t { *t = started } }).or_insert(started);
            }
        }
    }
    map
}

fn line_time(line: &str) -> Option<DateTime<Utc>> {
    let caps = LINE_TIME_RE.captures(line)?;
    DateTime::parse_from_rfc3339(&caps[1]).ok().map(|dt| dt.with_timezone(&Utc))
}

fn read_head(path: &std::path::Path, limit: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; limit];
    let read = file.read(&mut buf)?;
    buf.truncate(read);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "2026-10-10T03:42:01.534Z,1.5,76a0,6 [FLog::Output] ! Joining game 'e88d60e0-7feb-43a0-bbf8-039ad0ce07e3' place 6872265039 at 10.208.8.159\n2026-10-10T03:42:01.534Z,1.5,76a0,6 [FLog::GameJoinLoadTime] Report game_join_loadtime: placeid:6872265039, join_time:0.75, universeid:2619619496, referral_page:RequestGameJob\n2026-10-10T03:44:34.035Z,154.0,b870,6 [DFLog::NetworkClient] Client:Disconnect\n";

    #[test]
    fn parses_join_with_universe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x_last.log");
        std::fs::write(&path, SAMPLE).unwrap();
        let joins = parse_joins(&path);
        assert_eq!(joins.len(), 1);
        assert_eq!(joins[0].place_id, 6872265039);
        assert_eq!(joins[0].universe_id, 2619619496);
        assert_eq!(joins[0].job_id, "e88d60e0-7feb-43a0-bbf8-039ad0ce07e3");
        assert_eq!(joins[0].server_ip, "10.208.8.159");
        assert!(joins[0].at.is_some());
    }
}
