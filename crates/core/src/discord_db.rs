use crate::error::{Error, Result};
use crate::http::{Http, TIMEOUT, TIMEOUT_LONG};
use crate::paths::sanitize_relative_path;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const DISCORD_API_URL: &str = "https://discord.com/api/v9/applications/detectable";
pub const BACKUP_URL: &str = "https://gist.githubusercontent.com/Cynosphere/c1e77f77f0e565ddaac2822977961e76/raw/gameslist.json";
pub const MAX_SEARCH_RESULTS: usize = 20;

const DISCORD_HEADERS: &[(&str, &str)] = &[
    ("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
    ("Accept", "application/json"),
    ("Accept-Language", "en-US,en;q=0.9"),
    ("Referer", "https://discord.com/"),
    ("Origin", "https://discord.com"),
];

const SKIP_PATTERNS: &[&str] = &[
    "_be.exe", "_eac.exe", "launcher", "unins", "crash", "report", "update", "setup", "install",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutableEntry {
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameRecord {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    #[serde(default)]
    pub executables: Option<Vec<ExecutableEntry>>,
}

impl GameRecord {
    pub fn id(&self) -> &str {
        self.id.as_deref().unwrap_or("")
    }
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }
    pub fn aliases(&self) -> &[String] {
        self.aliases.as_deref().unwrap_or(&[])
    }

    fn win32_exes(&self, skip_patterns: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for exe in self.executables.as_deref().unwrap_or(&[]) {
            if exe.os.as_deref() != Some("win32") {
                continue;
            }
            let raw = exe.name.as_deref().unwrap_or("");
            let raw = raw.strip_prefix('>').unwrap_or(raw);
            let name = sanitize_relative_path(raw);
            if name.is_empty() || seen.contains(&name) {
                continue;
            }
            if skip_patterns {
                let lower = name.to_lowercase();
                if SKIP_PATTERNS.iter().any(|p| lower.contains(p)) {
                    continue;
                }
            }
            seen.insert(name.clone());
            out.push(name);
        }
        out
    }

    pub fn primary_executable(&self) -> Option<String> {
        self.win32_exes(true).into_iter().next()
    }

    pub fn all_executables(&self) -> Vec<String> {
        self.win32_exes(false)
    }
}

#[derive(Debug, Clone)]
pub struct DiscordDb {
    pub games: Vec<GameRecord>,
    pub source: String,
}

impl DiscordDb {
    pub fn from_games(games: Vec<GameRecord>, source: &str) -> Self {
        DiscordDb { games, source: source.to_string() }
    }

    pub fn load(http: &dyn Http) -> Result<DiscordDb> {
        let mut errors: Vec<String> = Vec::new();
        match fetch_games(http, DISCORD_API_URL, DISCORD_HEADERS, TIMEOUT) {
            Ok(games) => return Ok(Self::from_games(games, "Discord Official API")),
            Err(e) => errors.push(format!("Discord API: {e}")),
        }
        match fetch_games(http, BACKUP_URL, &[], TIMEOUT_LONG) {
            Ok(games) => return Ok(Self::from_games(games, "GitHub Backup")),
            Err(e) => errors.push(format!("GitHub backup: {e}")),
        }
        Err(Error::DatabaseLoad(format!(
            "Could not load the games database from any source. {}",
            errors.join(" | ")
        )))
    }

    pub fn search(&self, query: &str) -> Vec<&GameRecord> {
        let q = query.to_lowercase();
        let mut exact: Vec<&GameRecord> = Vec::new();
        let mut exact_ids: HashSet<&str> = HashSet::new();
        let mut partial: Vec<&GameRecord> = Vec::new();
        let mut partial_ids: HashSet<&str> = HashSet::new();
        for game in &self.games {
            let id = game.id();
            let name = game.name().to_lowercase();
            let aliases: Vec<String> = game.aliases().iter().map(|a| a.to_lowercase()).collect();
            if q == name || aliases.contains(&q) {
                if !id.is_empty() && exact_ids.insert(id) {
                    exact.push(game);
                }
            } else if (name.contains(&q) || aliases.iter().any(|a| a.contains(&q)))
                && !id.is_empty() && partial_ids.insert(id) {
                    partial.push(game);
                }
        }
        let mut merged = exact;
        merged.extend(partial.into_iter().filter(|g| !exact_ids.contains(g.id())));
        merged.truncate(MAX_SEARCH_RESULTS);
        merged
    }

    pub fn find_by_id(&self, id: &str) -> Option<&GameRecord> {
        self.games.iter().find(|g| g.id() == id)
    }
}

fn fetch_games(
    http: &dyn Http,
    url: &str,
    headers: &[(&str, &str)],
    timeout: u64,
) -> Result<Vec<GameRecord>> {
    let value = http.get_json(url, headers, &[], timeout)?;
    serde_json::from_value::<Vec<GameRecord>>(value)
        .map_err(|e| Error::Network(format!("Unexpected games list format: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Mutex;

    struct FakeHttp {
        calls: Mutex<Vec<String>>,
        discord: Option<Value>,
        backup: Option<Value>,
    }
    impl Http for FakeHttp {
        fn get_json(&self, url: &str, _: &[(&str, &str)], _: &[(&str, &str)], _: u64) -> Result<Value> {
            self.calls.lock().unwrap().push(url.to_string());
            let v = if url == DISCORD_API_URL { &self.discord } else { &self.backup };
            v.clone().ok_or_else(|| Error::Network("down".into()))
        }
    }

    fn games() -> Value {
        json!([
            {"id": "1", "name": "PUBG: Battlegrounds", "aliases": ["PUBG"],
             "executables": [{"os": "win32", "name": "TslGame.exe"}]},
            {"id": "2", "name": "PUBG", "aliases": [], "executables": []},
            {"id": "3", "name": "Other", "aliases": ["pubgish"]},
            {"name": "NoId PUBG"}
        ])
    }

    #[test]
    fn loads_from_discord_first() {
        let http = FakeHttp { calls: Mutex::new(vec![]), discord: Some(games()), backup: None };
        let db = DiscordDb::load(&http).unwrap();
        assert_eq!(db.source, "Discord Official API");
        assert_eq!(http.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn falls_back_to_backup() {
        let http = FakeHttp { calls: Mutex::new(vec![]), discord: None, backup: Some(games()) };
        let db = DiscordDb::load(&http).unwrap();
        assert_eq!(db.source, "GitHub Backup");
    }

    #[test]
    fn fails_when_both_sources_fail() {
        let http = FakeHttp { calls: Mutex::new(vec![]), discord: None, backup: None };
        assert!(matches!(DiscordDb::load(&http), Err(Error::DatabaseLoad(_))));
    }

    #[test]
    fn wrong_shape_counts_as_failure_and_falls_back() {
        let http = FakeHttp { calls: Mutex::new(vec![]), discord: Some(json!({"x": 1})), backup: Some(games()) };
        assert_eq!(DiscordDb::load(&http).unwrap().source, "GitHub Backup");
    }

    #[test]
    fn search_puts_exact_before_partial_and_skips_missing_ids() {
        let db = DiscordDb::from_games(serde_json::from_value(games()).unwrap(), "t");
        let ids: Vec<&str> = db.search("pubg").iter().map(|g| g.id()).collect();
        assert_eq!(ids, vec!["1", "2", "3"]);
    }

    #[test]
    fn search_is_case_insensitive_and_capped() {
        let many: Vec<GameRecord> = (0..50)
            .map(|i| GameRecord { id: Some(i.to_string()), name: Some(format!("Game {i}")), ..Default::default() })
            .collect();
        let db = DiscordDb::from_games(many, "t");
        assert_eq!(db.search("GAME").len(), MAX_SEARCH_RESULTS);
    }

    #[test]
    fn executables_skip_launchers_and_strip_prefix() {
        let g: GameRecord = serde_json::from_value(json!({
            "id": "9", "name": "X",
            "executables": [
                {"os": "win32", "name": "X_Launcher.exe"},
                {"os": "linux", "name": "x.bin"},
                {"os": "win32", "name": ">Bin\\X.exe"},
                {"os": "win32", "name": ">Bin\\X.exe"},
                {"os": "win32", "name": "x_be.exe"}
            ]
        })).unwrap();
        assert_eq!(g.primary_executable().as_deref(), Some("Bin/X.exe"));
        assert_eq!(g.all_executables(), vec!["X_Launcher.exe", "Bin/X.exe", "x_be.exe"]);
    }

    #[test]
    fn game_without_windows_exe_has_no_primary() {
        let g: GameRecord = serde_json::from_value(json!({"id": "1", "executables": [{"os": "darwin", "name": "x"}]})).unwrap();
        assert_eq!(g.primary_executable(), None);
    }
}
