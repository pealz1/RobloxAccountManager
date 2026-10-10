//! Session and crash logs with secret redaction (port of `diagnostics.py`).

use regex::Regex;
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

pub const DEFAULT_RETENTION: usize = 20;
const RECENT_LINES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub at: chrono::DateTime<chrono::Local>,
    pub level: Level,
    pub text: String,
}

struct Sink {
    file: Option<File>,
    path: Option<PathBuf>,
    recent: VecDeque<LogLine>,
}

static SINK: LazyLock<Mutex<Sink>> = LazyLock::new(|| Mutex::new(Sink { file: None, path: None, recent: VecDeque::new() }));

static REDACTIONS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?i)(\.ROBLOSECURITY\s*=\s*)[^;\s'\x22]+", "${1}[REDACTED]"),
        (r#"(?i)(["']?(?:cookie|password|auth_ticket|token)["']?\s*[:=]\s*["'])[^"']+"#, "${1}[REDACTED]"),
        (r"(?i)(privateServerLinkCode=|linkCode=|accessCode=)[^&\s'\x22]+", "${1}[REDACTED]"),
        (r"(?i)(link code:\s*)[A-Za-z0-9_-]+", "${1}[REDACTED]"),
        (r"(?i)(gameinfo:)[^+\s]+", "${1}[REDACTED]"),
        (r"(?i)(Bearer\s+)[A-Za-z0-9._-]+", "${1}[REDACTED]"),
        (r"https://(?:canary\.)?discord(?:app)?\.com/api/webhooks/[^\s'\x22]+", "[REDACTED WEBHOOK]"),
        (r"_\|WARNING:-DO-NOT-SHARE-THIS\.[^\s'\x22]+", "[REDACTED ROBLOX COOKIE]"),
    ]
    .into_iter()
    .map(|(pattern, replacement)| (Regex::new(pattern).expect("valid redaction regex"), replacement))
    .collect()
});

/// Removes cookies, passwords, link codes and tokens from text before it is stored.
pub fn redact(text: &str) -> String {
    REDACTIONS.iter().fold(text.to_owned(), |acc, (re, rep)| re.replace_all(&acc, *rep).into_owned())
}

/// Opens the session log, prunes old logs and installs the crash hook.
pub fn init(retention: usize) {
    let dir = crate::paths::logs_dir();
    let _ = fs::create_dir_all(&dir);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("session-{stamp}-{}.log", std::process::id()));
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    {
        let mut sink = SINK.lock().unwrap_or_else(|p| p.into_inner());
        sink.file = file;
        sink.path = Some(path.clone());
    }
    prune(retention, &path);
    install_panic_hook();
    write(Level::Info, &format!("{} {} started", crate::APP_NAME, crate::VERSION));
}

pub fn session_log_path() -> Option<PathBuf> {
    SINK.lock().ok().and_then(|sink| sink.path.clone())
}

pub fn write(level: Level, text: &str) {
    let clean = redact(text);
    let now = chrono::Local::now();
    let line = format!("{} [{}] {}\n", now.format("%Y-%m-%d %H:%M:%S%.3f"), level.tag(), clean);
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    let mut sink = SINK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(file) = sink.file.as_mut() {
        let _ = file.write_all(line.as_bytes());
    }
    if sink.recent.len() >= RECENT_LINES {
        sink.recent.pop_front();
    }
    sink.recent.push_back(LogLine { at: now, level, text: clean });
}

pub fn recent(limit: usize) -> Vec<LogLine> {
    let sink = SINK.lock().unwrap_or_else(|p| p.into_inner());
    let skip = sink.recent.len().saturating_sub(limit);
    sink.recent.iter().skip(skip).cloned().collect()
}

/// Keeps only the newest `keep` session and crash logs.
pub fn prune(keep: usize, protect: &PathBuf) {
    let dir = crate::paths::logs_dir();
    for prefix in ["session-", "crash-", "update-"] {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_, path) in files.into_iter().skip(keep.max(1)) {
            if &path != protect {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let text = format!("{info}\n\n{backtrace}");
        write(Level::Error, &format!("Panic: {info}"));
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = crate::paths::logs_dir().join(format!("crash-{stamp}.log"));
        let _ = fs::write(&path, redact(&text));
        previous(info);
    }));
}

#[macro_export]
macro_rules! log_info { ($($arg:tt)*) => { $crate::logging::write($crate::logging::Level::Info, &format!($($arg)*)) }; }
#[macro_export]
macro_rules! log_warn { ($($arg:tt)*) => { $crate::logging::write($crate::logging::Level::Warn, &format!($($arg)*)) }; }
#[macro_export]
macro_rules! log_error { ($($arg:tt)*) => { $crate::logging::write($crate::logging::Level::Error, &format!($($arg)*)) }; }

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_secrets() {
        let cookie = "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing|ABCDEF";
        assert!(!redact(&format!("cookie {cookie} end")).contains("ABCDEF"));
        assert_eq!(redact("Cookie: .ROBLOSECURITY=abc123; x"), "Cookie: .ROBLOSECURITY=[REDACTED]; x");
        assert!(!redact("https://x/games/1?privateServerLinkCode=9876").contains("9876"));
        assert!(!redact(r#"{"password": "hunter2"}"#).contains("hunter2"));
        assert!(!redact("roblox-player:1+gameinfo:SECRET+launchtime:1").contains("SECRET"));
        assert!(!redact("Authorization: Bearer abc.def").contains("abc.def"));
    }
}
