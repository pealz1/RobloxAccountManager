//! Data directory resolution (port of `app_paths.py`).
//!
//! Priority: `--data-dir <path>` > `NOVA_DATA_DIR` > `<exe dir>\NovaData`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const DATA_DIR_ENV: &str = "NOVA_DATA_DIR";
pub const DATA_DIR_FLAG: &str = "--data-dir";
const DEFAULT_DIR_NAME: &str = "NovaData";

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("NovaRAM.exe"))
}

pub fn exe_dir() -> PathBuf {
    exe_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Expands `~` and `%VAR%` and makes the path absolute.
pub fn expand(raw: &str) -> PathBuf {
    let mut text = raw.trim().trim_matches('"').to_owned();
    if let Some(rest) = text.strip_prefix('~') {
        if let Ok(home) = std::env::var("USERPROFILE") {
            text = format!("{home}{rest}");
        }
    }
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find('%') {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        match tail[1..].find('%') {
            Some(end) => {
                let name = &tail[1..=end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => out.push_str(&tail[..=end + 1]),
                }
                rest = &tail[end + 2..];
            }
            None => {
                out.push_str(tail);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    let path = PathBuf::from(out);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

/// Removes `--data-dir` from `args` and returns the chosen override, if any.
pub fn take_data_dir_arg(args: &mut Vec<String>) -> Option<String> {
    let mut found = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == DATA_DIR_FLAG {
            args.remove(index);
            if index < args.len() {
                found = Some(args.remove(index));
            }
        } else if let Some(value) = args[index].strip_prefix("--data-dir=") {
            found = Some(value.to_owned());
            args.remove(index);
        } else {
            index += 1;
        }
    }
    found.filter(|value| !value.trim().is_empty())
}

pub fn init_data_dir(cli_override: Option<String>) -> &'static Path {
    let chosen = cli_override
        .or_else(|| std::env::var(DATA_DIR_ENV).ok().filter(|v| !v.trim().is_empty()))
        .map(|raw| expand(&raw))
        .unwrap_or_else(|| exe_dir().join(DEFAULT_DIR_NAME));
    let _ = std::fs::create_dir_all(&chosen);
    DATA_DIR.get_or_init(|| chosen)
}

pub fn data_dir() -> &'static Path {
    DATA_DIR.get_or_init(|| {
        let dir = exe_dir().join(DEFAULT_DIR_NAME);
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
}

pub fn data_file(name: &str) -> PathBuf {
    data_dir().join(name)
}

pub fn logs_dir() -> PathBuf {
    data_file("logs")
}

pub fn local_appdata() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_flag_is_removed_from_args() {
        let mut args = vec!["app".into(), "--data-dir".into(), "D:/x".into(), "--mcp".into()];
        assert_eq!(take_data_dir_arg(&mut args).as_deref(), Some("D:/x"));
        assert_eq!(args, vec!["app".to_string(), "--mcp".to_string()]);
        let mut args = vec!["app".into(), "--data-dir=E:/y".into()];
        assert_eq!(take_data_dir_arg(&mut args).as_deref(), Some("E:/y"));
    }

    #[test]
    fn expands_environment_variables() {
        unsafe { std::env::set_var("NOVA_TEST_VAR", "C:\\Profiles") };
        assert_eq!(expand("%NOVA_TEST_VAR%\\Alt"), PathBuf::from("C:\\Profiles\\Alt"));
        assert!(expand("%NOPE_NOT_SET%\\x").to_string_lossy().contains("%NOPE_NOT_SET%"));
    }
}
