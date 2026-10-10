//! Avatar thumbnails: fetch from Roblox, cache on disk, refresh when stale.

use crate::roblox::{agent, get_json};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const SIZE: &str = "150x150";

#[derive(Deserialize)]
struct ThumbResponse {
    data: Vec<ThumbEntry>,
}

#[derive(Deserialize)]
struct ThumbEntry {
    #[serde(rename = "targetId")]
    target_id: u64,
    #[serde(default)]
    state: String,
    #[serde(rename = "imageUrl", default)]
    image_url: String,
}

/// Fetches headshot image URLs for up to 100 user ids at once.
pub fn fetch_urls(user_ids: &[u64]) -> HashMap<u64, String> {
    if user_ids.is_empty() {
        return HashMap::new();
    }
    let ids = user_ids.iter().take(100).map(u64::to_string).collect::<Vec<_>>().join(",");
    let url = format!("https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={ids}&size={SIZE}&format=Png&isCircular=false");
    match get_json::<ThumbResponse>("Avatars", &url, None) {
        Ok(response) => response
            .data
            .into_iter()
            .filter(|e| e.state == "Completed" && !e.image_url.is_empty())
            .map(|e| (e.target_id, e.image_url))
            .collect(),
        Err(_) => HashMap::new(),
    }
}

fn download(url: &str) -> Option<Vec<u8>> {
    let mut response = agent().get(url).call().ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    let bytes = response.body_mut().read_to_vec().ok()?;
    (bytes.len() <= 4 * 1024 * 1024).then_some(bytes)
}

/// On-disk + in-memory avatar cache keyed by user id.
pub struct AvatarCache {
    dir: PathBuf,
    max_age: Duration,
    memory: Mutex<HashMap<u64, Arc<Vec<u8>>>>,
}

impl AvatarCache {
    pub fn new(cache_days: u32) -> AvatarCache {
        Self::with_dir(crate::paths::data_file("avatar_cache"), cache_days)
    }

    pub fn with_dir(dir: PathBuf, cache_days: u32) -> AvatarCache {
        let _ = std::fs::create_dir_all(&dir);
        AvatarCache { dir, max_age: Duration::from_secs(cache_days as u64 * 86_400), memory: Mutex::new(HashMap::new()) }
    }

    fn path(&self, user_id: u64) -> PathBuf {
        self.dir.join(format!("{user_id}.png"))
    }

    fn is_stale(&self, path: &std::path::Path) -> bool {
        if self.max_age.is_zero() {
            return false; // 0 = keep forever
        }
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map(|modified| SystemTime::now().duration_since(modified).unwrap_or_default() > self.max_age)
            .unwrap_or(true)
    }

    /// Cached PNG bytes if present (ignores staleness; used to show something immediately).
    pub fn cached(&self, user_id: u64) -> Option<Arc<Vec<u8>>> {
        if let Some(bytes) = self.memory.lock().unwrap_or_else(|p| p.into_inner()).get(&user_id) {
            return Some(Arc::clone(bytes));
        }
        let bytes = std::fs::read(self.path(user_id)).ok()?;
        let shared = Arc::new(bytes);
        self.memory.lock().unwrap_or_else(|p| p.into_inner()).insert(user_id, Arc::clone(&shared));
        Some(shared)
    }

    pub fn needs_refresh(&self, user_id: u64) -> bool {
        let path = self.path(user_id);
        !path.exists() || self.is_stale(&path)
    }

    fn store(&self, user_id: u64, bytes: Vec<u8>) -> Arc<Vec<u8>> {
        let _ = std::fs::write(self.path(user_id), &bytes);
        let shared = Arc::new(bytes);
        self.memory.lock().unwrap_or_else(|p| p.into_inner()).insert(user_id, Arc::clone(&shared));
        shared
    }

    /// Downloads and caches avatars for the given ids that are missing or stale.
    /// Returns the ids that were (re)fetched. Runs the network on the calling thread.
    pub fn refresh(&self, user_ids: &[u64]) -> Vec<u64> {
        let wanted: Vec<u64> = user_ids.iter().copied().filter(|id| *id > 0 && self.needs_refresh(*id)).collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        let urls = fetch_urls(&wanted);
        let mut done = Vec::new();
        for (id, url) in urls {
            if let Some(bytes) = download(&url) {
                self.store(id, bytes);
                done.push(id);
            }
        }
        done
    }

    /// Removes cached avatars for ids no longer in use.
    pub fn prune(&self, keep: &std::collections::HashSet<u64>) {
        for entry in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            if let Some(id) = entry.path().file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok())
                && !keep.contains(&id)
            {
                let _ = std::fs::remove_file(entry.path());
                self.memory.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_stores_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = AvatarCache::with_dir(dir.path().to_path_buf(), 7);
        cache.store(42, vec![1, 2, 3]);
        assert_eq!(cache.cached(42).unwrap().as_slice(), &[1, 2, 3]);
        assert!(!cache.needs_refresh(42));
        cache.prune(&std::collections::HashSet::new());
        assert!(cache.cached(42).is_none());
    }
}
