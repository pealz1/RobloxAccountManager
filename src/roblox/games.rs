//! Game lookups: name resolution, search for the game picker, and small-server finding.

use super::get_json;
use crate::error::{AppError, AppResult};
use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameInfo {
    pub universe_id: u64,
    pub place_id: u64,
    pub name: String,
    pub icon_url: String,
}

#[derive(Deserialize)]
struct UniverseResponse {
    #[serde(rename = "universeId")]
    universe_id: Option<u64>,
}

#[derive(Deserialize)]
struct GamesResponse {
    data: Vec<GameEntry>,
}

#[derive(Deserialize)]
struct GameEntry {
    #[serde(default)]
    name: String,
}

/// Resolves a place id to its game name (empty when unknown).
pub fn game_name(place_id: u64) -> AppResult<String> {
    let universe: UniverseResponse =
        get_json("Game lookup", &format!("https://apis.roblox.com/universes/v1/places/{place_id}/universe"), None)?;
    let Some(universe_id) = universe.universe_id else {
        return Ok(String::new());
    };
    let games: GamesResponse = get_json("Game lookup", &format!("https://games.roblox.com/v1/games?universeIds={universe_id}"), None)?;
    Ok(games.data.into_iter().next().map(|g| g.name).unwrap_or_default())
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(rename = "searchResults", default)]
    groups: Vec<SearchGroup>,
    #[serde(rename = "nextPageToken", default)]
    next_page: Option<String>,
}

#[derive(Deserialize)]
struct SearchGroup {
    #[serde(rename = "contentGroupType", default)]
    group_type: String,
    #[serde(default)]
    contents: Vec<SearchGame>,
}

#[derive(Deserialize)]
struct SearchGame {
    #[serde(rename = "universeId", default)]
    universe_id: Option<serde_json::Value>,
    #[serde(rename = "rootPlaceId", default)]
    root_place_id: Option<serde_json::Value>,
    #[serde(default)]
    name: String,
}

fn as_id(value: &Option<serde_json::Value>) -> u64 {
    value.as_ref().and_then(crate::import::as_u64).unwrap_or(0)
}

/// Searches games by name for the game picker. `session_id` groups a user's requests.
pub fn search(query: &str, session_id: &str, page_token: &str) -> AppResult<(Vec<GameInfo>, String)> {
    let query = query.trim();
    if query.is_empty() {
        return Ok((Vec::new(), String::new()));
    }
    let mut url =
        format!("https://apis.roblox.com/search-api/omni-search?searchQuery={}&sessionId={session_id}&pageType=all", urlencode(query));
    if !page_token.is_empty() {
        url.push_str(&format!("&pageToken={}", urlencode(page_token)));
    }
    let response: SearchResponse = get_json("Game search", &url, None)?;
    let mut games = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for group in &response.groups {
        if group.group_type != "Game" {
            continue;
        }
        for game in &group.contents {
            let universe_id = as_id(&game.universe_id);
            let place_id = as_id(&game.root_place_id);
            if universe_id == 0 || place_id == 0 || game.name.trim().is_empty() {
                continue;
            }
            if seen.insert(universe_id) {
                games.push(GameInfo { universe_id, place_id, name: game.name.trim().to_owned(), icon_url: String::new() });
            }
        }
    }
    Ok((games, response.next_page.unwrap_or_default()))
}

#[derive(Deserialize)]
struct ServerList {
    data: Vec<ServerEntry>,
}

#[derive(Deserialize)]
struct ServerEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    playing: u32,
    #[serde(rename = "maxPlayers", default)]
    max_players: u32,
}

/// Finds the public server with the fewest players that still has room.
pub fn smallest_server(place_id: u64) -> AppResult<String> {
    let url = format!("https://games.roblox.com/v1/games/{place_id}/servers/Public?sortOrder=Asc&limit=100");
    let list: ServerList = get_json("Server list", &url, None)?;
    list.data
        .into_iter()
        .filter(|s| !s.id.is_empty() && s.playing < s.max_players.max(1))
        .min_by_key(|s| s.playing)
        .map(|s| s.id)
        .ok_or_else(|| {
            AppError::new("NO_JOINABLE_SERVER", "No Available Server", "No joinable public server was found for this game.").retryable()
        })
}

fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_escapes_spaces_and_symbols() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }
}
