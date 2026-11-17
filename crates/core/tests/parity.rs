use orbshacker_core::discord_db::{DiscordDb, GameRecord};
use orbshacker_core::paths::*;
use orbshacker_core::steam::*;
use serde_json::Value;
use std::path::Path;

fn fixtures() -> Value {
    let text = include_str!("fixtures/parity.json");
    serde_json::from_str(text).expect("parity.json is valid")
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

#[test]
fn filenames_match_python() {
    let f = fixtures();
    for case in f["filename"].as_array().unwrap() {
        assert_eq!(sanitize_filename(s(&case[0])), s(&case[1]), "filename {:?}", case[0]);
    }
    for case in f["segment"].as_array().unwrap() {
        assert_eq!(sanitize_path_segment(s(&case[0])), s(&case[1]), "segment {:?}", case[0]);
    }
}

#[test]
fn relative_paths_match_python() {
    for case in fixtures()["relative"].as_array().unwrap() {
        assert_eq!(sanitize_relative_path(s(&case[0])), s(&case[1]), "path {:?}", case[0]);
    }
}

#[test]
fn search_matches_python() {
    let f = fixtures();
    let games: Vec<GameRecord> = serde_json::from_value(f["games"].clone()).unwrap();
    let db = DiscordDb::from_games(games, "t");
    for case in f["search"].as_array().unwrap() {
        let got: Vec<String> = db.search(s(&case[0])).iter().map(|g| g.id().to_string()).collect();
        let want: Vec<String> = case[1].as_array().unwrap().iter().map(|v| s(v).to_string()).collect();
        assert_eq!(got, want, "query {:?}", case[0]);
    }
}

#[test]
fn executables_match_python() {
    let f = fixtures();
    let games: Vec<GameRecord> = serde_json::from_value(f["games"].clone()).unwrap();
    for (game, case) in games.iter().zip(f["exes"].as_array().unwrap()) {
        let primary = case[1].as_str().map(str::to_string);
        let all: Vec<String> = case[2].as_array().unwrap().iter().map(|v| s(v).to_string()).collect();
        assert_eq!(game.primary_executable(), primary, "primary of {:?}", game.id());
        assert_eq!(game.all_executables(), all, "all of {:?}", game.id());
    }
}

#[test]
fn steam_launch_maps_match_python() {
    let f = fixtures();
    for case in f["pick_exe"].as_array().unwrap() {
        let got = pick_windows_exe(case[0].as_object().unwrap());
        assert_eq!(got.as_deref(), case[1].as_str(), "pick {:?}", case[0]);
    }
    for case in f["resolve"].as_array().unwrap() {
        let got = resolve_executable(1, "Some Game", case[0].as_object().unwrap());
        assert_eq!(got, s(&case[1]), "resolve {:?}", case[0]);
    }
}

#[test]
fn steam_info_matches_python() {
    for case in fixtures()["steam_info"].as_array().unwrap() {
        let appid = case[0].as_u64().unwrap();
        let got = parse_app_info(appid, &case[1]);
        let want = &case[2];
        assert_eq!(got.name, s(&want["name"]), "name of {appid}");
        assert_eq!(got.installdir, s(&want["installdir"]), "installdir of {appid}");
        assert_eq!(got.executable, s(&want["executable"]), "exe of {appid}");
        assert_eq!(got.depot_id.as_deref(), want["depot_id"].as_str(), "depot of {appid}");
    }
}

#[test]
fn appmanifest_matches_python_byte_for_byte() {
    for case in fixtures()["acf"].as_array().unwrap() {
        let got = render_appmanifest(
            case[0].as_u64().unwrap(),
            s(&case[1]),
            s(&case[2]),
            Path::new(s(&case[3])),
            s(&case[4]),
            case[5].as_str(),
        );
        assert_eq!(got, s(&case[6]), "acf for {:?}", case[3]);
    }
}
