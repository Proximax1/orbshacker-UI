use crate::error::{Error, Result};
use crate::http::Http;
use crate::paths::{sanitize_filename, sanitize_path_segment, sanitize_relative_path};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const STEAMCMD_API_URL: &str = "https://api.steamcmd.net/v1/info";
pub const STORE_SEARCH_URL: &str = "https://store.steampowered.com/api/storesearch";
const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SteamAppInfo {
    pub name: String,
    pub installdir: String,
    pub executable: String,
    pub depot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SteamStoreItem {
    pub id: u64,
    pub name: String,
}


#[cfg(windows)]
pub fn steam_path() -> Option<PathBuf> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam");
    if let Ok(key) = key {
        if let Ok(v) = key.get_value::<String, _>("SteamPath") {
            return Some(PathBuf::from(v));
        }
    }
    let fallback = PathBuf::from("C:/Program Files (x86)/Steam");
    fallback.exists().then_some(fallback)
}

#[cfg(not(windows))]
pub fn steam_path() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
pub fn steam_user_id() -> String {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam\ActiveProcess")
        .ok()
        .and_then(|k| k.get_value::<u32, _>("ActiveUser").ok())
        .map(|v| (v as u64 + STEAM_ID64_BASE).to_string())
        .unwrap_or_else(|| "0".into())
}

#[cfg(not(windows))]
pub fn steam_user_id() -> String {
    "0".into()
}


pub fn pick_windows_exe(launch: &serde_json::Map<String, Value>) -> Option<String> {
    let mut keys: Vec<&String> = launch.keys().collect();
    keys.sort();
    for key in keys {
        let entry = &launch[key];
        let oslist = entry
            .get("config")
            .and_then(|c| c.get("oslist"))
            .and_then(Value::as_str)
            .unwrap_or("windows");
        if oslist.contains("windows") || oslist.is_empty() {
            let exe = entry.get("executable").and_then(Value::as_str).unwrap_or("");
            if exe.ends_with(".exe") {
                return Some(exe.replace('\\', "/"));
            }
        }
    }
    None
}

pub fn resolve_executable(
    appid: u64,
    installdir: &str,
    launch: &serde_json::Map<String, Value>,
) -> String {
    if appid == 4_080_220 {
        return "fc27.exe".into();
    }
    let executable = pick_windows_exe(launch)
        .unwrap_or_else(|| format!("{}.exe", installdir.rsplit('/').next().unwrap_or("")));
    sanitize_relative_path(&executable)
}

pub fn parse_app_info(appid: u64, data: &Value) -> SteamAppInfo {
    let app = data
        .get("data")
        .and_then(|d| d.get(appid.to_string()))
        .cloned()
        .unwrap_or(Value::Null);
    let raw_name = app
        .get("common")
        .and_then(|c| c.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("App {appid}"));
    let name = sanitize_filename(&raw_name);
    let raw_installdir = app
        .get("config")
        .and_then(|c| c.get("installdir"))
        .and_then(Value::as_str)
        .unwrap_or(&raw_name)
        .to_string();
    let mut installdir = sanitize_path_segment(&raw_installdir);
    if installdir.is_empty() {
        installdir = name.clone();
    }
    let empty = serde_json::Map::new();
    let launch = app
        .get("config")
        .and_then(|c| c.get("launch"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let executable = resolve_executable(appid, &installdir, launch);
    let depot_id = app
        .get("depots")
        .and_then(Value::as_object)
        .and_then(|d| {
            d.keys()
                .find(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_digit()))
                .cloned()
        });
    SteamAppInfo { name, installdir, executable, depot_id }
}

pub fn fetch_app_info(http: &dyn Http, appid: u64) -> Result<SteamAppInfo> {
    let url = format!("{STEAMCMD_API_URL}/{appid}");
    let data = http.get_json(&url, &[], &[], crate::http::TIMEOUT)?;
    Ok(parse_app_info(appid, &data))
}

pub fn search_games(http: &dyn Http, query: &str) -> Result<Vec<SteamStoreItem>> {
    let data = http.get_json(
        STORE_SEARCH_URL,
        &[],
        &[("term", query), ("l", "english"), ("cc", "US")],
        crate::http::TIMEOUT,
    )?;
    let items = data.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(items
        .into_iter()
        .filter_map(|i| {
            Some(SteamStoreItem {
                id: i.get("id")?.as_u64()?,
                name: i.get("name")?.as_str()?.to_string(),
            })
        })
        .collect())
}


pub fn render_appmanifest(
    appid: u64,
    name: &str,
    installdir: &str,
    steam_path: &Path,
    owner: &str,
    depot_id: Option<&str>,
) -> String {
    let launcher = format!("{}\\steam.exe", steam_path.to_string_lossy().trim_end_matches(['/', '\\']))
        .replace('/', "\\");
    let staged = depot_id
        .map(|d| {
            format!(
                "\n\t\t\"{d}\"\n\t\t{{\n\t\t\t\"manifest\"\t\t\"0\"\n\t\t\t\"size\"\t\t\"1073741824\"\n\t\t\t\"dlcappid\"\t\t\"0\"\n\t\t}}"
            )
        })
        .unwrap_or_default();
    format!(
        "\"AppState\"\n{{\n\
\t\"appid\"\t\t\"{appid}\"\n\
\t\"universe\"\t\t\"1\"\n\
\t\"LauncherPath\"\t\t\"{launcher}\"\n\
\t\"name\"\t\t\"{name}\"\n\
\t\"StateFlags\"\t\t\"1026\"\n\
\t\"installdir\"\t\t\"{installdir}\"\n\
\t\"LastUpdated\"\t\t\"0\"\n\
\t\"LastPlayed\"\t\t\"0\"\n\
\t\"SizeOnDisk\"\t\t\"0\"\n\
\t\"StagingSize\"\t\t\"1073741824\"\n\
\t\"buildid\"\t\t\"0\"\n\
\t\"LastOwner\"\t\t\"{owner}\"\n\
\t\"DownloadType\"\t\t\"1\"\n\
\t\"UpdateResult\"\t\t\"4\"\n\
\t\"BytesToDownload\"\t\t\"1073741824\"\n\
\t\"BytesDownloaded\"\t\t\"27262976\"\n\
\t\"BytesToStage\"\t\t\"1073741824\"\n\
\t\"BytesStaged\"\t\t\"27262976\"\n\
\t\"TargetBuildID\"\t\t\"0\"\n\
\t\"AutoUpdateBehavior\"\t\t\"0\"\n\
\t\"AllowOtherDownloadsWhileRunning\"\t\t\"0\"\n\
\t\"ScheduledAutoUpdate\"\t\t\"0\"\n\
\t\"InstalledDepots\"\n\
\t{{\n\
\t}}\n\
\t\"StagedDepots\"\n\
\t{{{staged}\n\
\t}}\n\
\t\"UserConfig\"\n\
\t{{\n\
\t}}\n\
\t\"MountedConfig\"\n\
\t{{\n\
\t}}\n\
}}\n"
    )
}

pub fn manifest_path(steam_path: &Path, appid: u64) -> PathBuf {
    steam_path.join("steamapps").join(format!("appmanifest_{appid}.acf"))
}

pub fn fake_exe_path(steam_path: &Path, installdir: &str, executable: &str) -> PathBuf {
    let mut p = steam_path.join("steamapps").join("common");
    p.push(sanitize_path_segment(installdir));
    for seg in sanitize_relative_path(executable).split('/') {
        p.push(seg);
    }
    p
}

pub fn is_our_manifest(text: &str) -> bool {
    text.contains("\"StateFlags\"\t\t\"1026\"")
        && text.contains("\"BytesDownloaded\"\t\t\"27262976\"")
        && text.contains("\"SizeOnDisk\"\t\t\"0\"")
}

pub fn ensure_manifest_safe(path: &Path) -> Result<()> {
    if let Ok(text) = std::fs::read_to_string(path) {
        if !is_our_manifest(&text) {
            return Err(Error::Invalid(format!(
                "{} belongs to a real Steam install. It was left untouched.",
                path.display()
            )));
        }
    } else if path.exists() {
        return Err(Error::Invalid(format!("{} exists but cannot be read.", path.display())));
    }
    Ok(())
}

pub fn write_appmanifest(
    appid: u64,
    info: &SteamAppInfo,
    steam_path: &Path,
) -> Result<PathBuf> {
    let acf = render_appmanifest(
        appid,
        &info.name,
        &info.installdir,
        steam_path,
        &steam_user_id(),
        info.depot_id.as_deref(),
    );
    let path = manifest_path(steam_path, appid);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, acf).map_err(Error::Io)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn launch(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn picks_first_sorted_windows_exe() {
        let l = launch(json!({
            "2": {"executable": "b.exe"},
            "1": {"executable": "bin\\a.exe", "config": {"oslist": "windows"}},
            "0": {"executable": "linux.sh", "config": {"oslist": "linux"}}
        }));
        assert_eq!(pick_windows_exe(&l).as_deref(), Some("bin/a.exe"));
    }

    #[test]
    fn skips_non_windows_and_non_exe() {
        let l = launch(json!({
            "0": {"executable": "run.sh"},
            "1": {"executable": "mac.exe", "config": {"oslist": "macos"}}
        }));
        assert_eq!(pick_windows_exe(&l), None);
    }

    #[test]
    fn empty_oslist_counts_as_windows() {
        let l = launch(json!({"0": {"executable": "a.exe", "config": {"oslist": ""}}}));
        assert_eq!(pick_windows_exe(&l).as_deref(), Some("a.exe"));
    }

    #[test]
    fn fc27_is_special_cased() {
        assert_eq!(resolve_executable(4_080_220, "FC", &serde_json::Map::new()), "fc27.exe");
    }

    #[test]
    fn falls_back_to_installdir_name() {
        assert_eq!(resolve_executable(1, "Some Game", &serde_json::Map::new()), "Some Game.exe");
    }

    #[test]
    fn parses_steamcmd_response() {
        let data = json!({"data": {"7": {
            "common": {"name": "Cool™ Game"},
            "config": {"installdir": "Cool Game", "launch": {"0": {"executable": "Bin\\cool.exe"}}},
            "depots": {"baselanguages": "x", "8": {}, "9": {}}
        }}});
        let info = parse_app_info(7, &data);
        assert_eq!(info.name, "Cool Game");
        assert_eq!(info.installdir, "Cool Game");
        assert_eq!(info.executable, "Bin/cool.exe");
        assert_eq!(info.depot_id.as_deref(), Some("8"));
    }

    #[test]
    fn parses_empty_response_with_defaults() {
        let info = parse_app_info(5, &json!({}));
        assert_eq!(info.name, "App 5");
        assert_eq!(info.installdir, "App 5");
        assert_eq!(info.executable, "App 5.exe");
        assert_eq!(info.depot_id, None);
    }

    #[test]
    fn manifest_contains_required_fields() {
        let acf = render_appmanifest(7, "Cool", "Cool", Path::new("C:/Steam"), "123", Some("8"));
        assert!(acf.starts_with("\"AppState\"\n{\n"));
        assert!(acf.contains("\t\"appid\"\t\t\"7\"\n"));
        assert!(acf.contains("\"StateFlags\"\t\t\"1026\""));
        assert!(acf.contains("\"LastOwner\"\t\t\"123\""));
        assert!(acf.contains("\"LauncherPath\"\t\t\"C:\\Steam\\steam.exe\""));
        assert!(acf.contains("\"StagedDepots\"\n\t{\n\t\t\"8\""));
        assert!(acf.ends_with("}\n"));
    }

    #[test]
    fn recognises_only_its_own_manifest() {
        let ours = render_appmanifest(7, "Cool", "Cool", Path::new("C:/Steam"), "1", None);
        assert!(is_our_manifest(&ours));
        let real = "\"AppState\"\n{\n\t\"appid\"\t\t\"7\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"SizeOnDisk\"\t\t\"5000\"\n}\n";
        assert!(!is_our_manifest(real));
    }

    #[test]
    fn ensure_manifest_safe_allows_missing_and_ours_but_not_real() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("appmanifest_7.acf");
        assert!(ensure_manifest_safe(&p).is_ok());
        std::fs::write(&p, render_appmanifest(7, "C", "C", Path::new("C:/S"), "1", None)).unwrap();
        assert!(ensure_manifest_safe(&p).is_ok());
        std::fs::write(&p, "\"AppState\" { \"StateFlags\" \"4\" }").unwrap();
        assert!(ensure_manifest_safe(&p).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "\"AppState\" { \"StateFlags\" \"4\" }");
    }

    #[test]
    fn manifest_without_depot_has_empty_staged_block() {
        let acf = render_appmanifest(7, "Cool", "Cool", Path::new("C:/Steam"), "0", None);
        assert!(acf.contains("\"StagedDepots\"\n\t{\n\t}\n"));
    }

    #[test]
    fn fake_exe_path_is_inside_steamapps_common() {
        let p = fake_exe_path(Path::new("C:/Steam"), "a/b", "..\\Bin/g.exe");
        let s = p.to_string_lossy().replace('\\', "/");
        assert_eq!(s, "C:/Steam/steamapps/common/ab/Bin/g.exe");
    }
}
