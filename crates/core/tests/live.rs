use orbshacker_core::discord_db::{DiscordDb, DISCORD_API_URL};
use orbshacker_core::http::{Http, UreqHttp};
use orbshacker_core::steam;
use std::time::Instant;

#[test]
#[ignore]
fn discord_api_returns_a_parsable_games_list() {
    let t = Instant::now();
    let db = DiscordDb::load(&UreqHttp).expect("a games source is reachable");
    println!("source={} games={} in {:?}", db.source, db.games.len(), t.elapsed());
    assert!(db.games.len() > 1000, "expected a big list, got {}", db.games.len());
    let hits = db.search("minecraft");
    assert!(!hits.is_empty(), "minecraft should be in the list");
    let with_exe = hits.iter().filter(|g| g.primary_executable().is_some()).count();
    println!("minecraft hits={} with primary exe={}", hits.len(), with_exe);
}

#[test]
#[ignore]
fn discord_api_direct() {
    let t = Instant::now();
    let r = UreqHttp.get_json(DISCORD_API_URL, &[("User-Agent", "Mozilla/5.0"), ("Accept", "application/json")], &[], 20);
    println!("direct discord call: {:?} in {:?}", r.as_ref().map(|v| v.as_array().map(|a| a.len())), t.elapsed());
}

#[test]
#[ignore]
fn steam_search_and_info_work() {
    let items = steam::search_games(&UreqHttp, "Portal 2").expect("steam store search");
    println!("steam results: {}", items.len());
    assert!(!items.is_empty());
    let first = &items[0];
    let info = steam::fetch_app_info(&UreqHttp, first.id).expect("steamcmd info");
    println!("{} -> {:?}", first.name, info);
    assert!(info.executable.ends_with(".exe"));
}
