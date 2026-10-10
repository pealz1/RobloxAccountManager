//! Applies the basic Roblox settings presets (framerate cap, master volume, start
//! quality) to `GlobalBasicSettings_13.xml` before a client launches.
//!
//! Only the three well-known keys are touched, and only when their preset is
//! enabled. Values are substituted in place for whatever element type Roblox used
//! (`<int>`, `<token>`, `<float>`, …); a key Roblox has not written yet is left
//! alone rather than guessed at. The full searchable advanced editor is not ported.

use crate::error::{AppError, AppResult};
use crate::store::settings::RobloxPresets;
use regex::Regex;
use std::path::PathBuf;

const FILE_NAME: &str = "GlobalBasicSettings_13.xml";

/// The settings file to edit: the custom path if set, else the default location.
pub fn settings_path(presets: &RobloxPresets) -> Option<PathBuf> {
    let custom = presets.settings_path.trim();
    if !custom.is_empty() {
        return Some(PathBuf::from(custom));
    }
    crate::paths::local_appdata().map(|p| p.join("Roblox").join(FILE_NAME))
}

/// Replaces the inner value of `<type name="KEY">value</type>` for the given key,
/// keeping Roblox's own element type. Returns the edited XML (unchanged if absent).
fn set_value(xml: &str, key: &str, value: &str) -> String {
    // The regex crate has no backreferences, so the closing tag is matched generically.
    // Values never contain '<', so the non-greedy body cannot span into another element.
    let pattern = format!(r#"(<(?:bool|int|int64|float|double|token|string)\s+name="{}">)[^<]*(</[A-Za-z0-9]+>)"#, regex::escape(key));
    match Regex::new(&pattern) {
        Ok(re) => re.replace(xml, |caps: &regex::Captures| format!("{}{}{}", &caps[1], value, &caps[2])).into_owned(),
        Err(_) => xml.to_owned(),
    }
}

fn has_key(xml: &str, key: &str) -> bool {
    xml.contains(&format!("name=\"{key}\""))
}

/// Applies the enabled basic presets. Best-effort: returns Ok and logs when the
/// file is missing or a key isn't present, so it never blocks a launch.
pub fn apply_presets(presets: &RobloxPresets) -> AppResult<()> {
    if !presets.framerate_cap_enabled && !presets.master_volume_enabled && !presets.graphics_quality_enabled {
        return Ok(());
    }
    let Some(path) = settings_path(presets) else {
        return Ok(());
    };
    if !path.exists() {
        crate::log_warn!("Roblox settings file not found; presets not applied ({})", path.display());
        return Ok(());
    }
    let original = std::fs::read_to_string(&path).map_err(|e| AppError::io("Reading Roblox settings", &e))?;
    let mut xml = original.clone();
    if presets.framerate_cap_enabled {
        apply_one(&mut xml, "FramerateCap", &presets.framerate_cap.to_string());
    }
    if presets.master_volume_enabled {
        // Roblox stores master volume as 0.0–1.0.
        apply_one(&mut xml, "MasterVolume", &format!("{:.4}", presets.master_volume.clamp(0.0, 1.0)));
    }
    if presets.graphics_quality_enabled {
        apply_one(&mut xml, "SavedQualityLevel", &presets.graphics_quality.min(10).to_string());
    }
    if xml == original {
        return Ok(());
    }
    write_possibly_readonly(&path, &xml)
}

fn apply_one(xml: &mut String, key: &str, value: &str) {
    if has_key(xml, key) {
        *xml = set_value(xml, key, value);
    } else {
        crate::log_warn!("Roblox settings has no '{key}' entry yet; skipped. Launch Roblox once to create it.");
    }
}

/// Writes the file, temporarily clearing the read-only flag if Roblox set it.
#[allow(clippy::permissions_set_readonly_false)] // we restore the flag afterward
fn write_possibly_readonly(path: &std::path::Path, contents: &str) -> AppResult<()> {
    let was_readonly = std::fs::metadata(path).map(|m| m.permissions().readonly()).unwrap_or(false);
    if was_readonly {
        let mut perms = std::fs::metadata(path).map_err(|e| AppError::io("Reading Roblox settings", &e))?.permissions();
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(path, perms);
    }
    let result = crate::store::atomic::write_atomic(path, contents.as_bytes()).map_err(|e| AppError::io("Writing Roblox settings", &e));
    if was_readonly && let Ok(mut perms) = std::fs::metadata(path).map(|m| m.permissions()) {
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(path, perms);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<roblox><Item class="UserGameSettings"><Properties>
<int name="FramerateCap">60</int>
<float name="MasterVolume">1</float>
<token name="SavedQualityLevel">0</token>
</Properties></Item></roblox>"#;

    #[test]
    fn sets_known_values_keeping_element_type() {
        let xml = set_value(SAMPLE, "FramerateCap", "240");
        assert!(xml.contains(r#"<int name="FramerateCap">240</int>"#));
        let xml = set_value(&xml, "MasterVolume", "0.5000");
        assert!(xml.contains(r#"<float name="MasterVolume">0.5000</float>"#));
        let xml = set_value(&xml, "SavedQualityLevel", "5");
        assert!(xml.contains(r#"<token name="SavedQualityLevel">5</token>"#));
    }

    #[test]
    fn missing_key_leaves_xml_unchanged() {
        assert_eq!(set_value(SAMPLE, "NotThere", "9"), SAMPLE);
        assert!(!has_key(SAMPLE, "NotThere"));
        assert!(has_key(SAMPLE, "FramerateCap"));
    }

    #[test]
    fn apply_presets_is_noop_when_all_disabled() {
        assert!(apply_presets(&RobloxPresets::default()).is_ok());
    }
}
