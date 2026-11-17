use orbshacker_core::baked;
use orbshacker_core::fake;
use orbshacker_core::session::{now_unix, Kind, NewSession, Sessions, Starter, State};
use orbshacker_core::settings::Settings;
use orbshacker_core::steam::{self, SteamAppInfo};
use std::path::PathBuf;

fn info() -> SteamAppInfo {
    SteamAppInfo { name: "Cool Game".into(), installdir: "Cool Game".into(), executable: "Bin/cool.exe".into(), depot_id: Some("8".into()) }
}

#[test]
fn manifest_and_fake_appear_on_their_turn_and_are_fully_removed() {
    let root = tempfile::tempdir().unwrap();
    let steam_dir = root.path().join("Steam");
    std::fs::create_dir_all(steam_dir.join("steamapps").join("common")).unwrap();
    let source = root.path().join("source.exe");
    std::fs::write(&source, vec![1u8; 4096]).unwrap();

    let info = info();
    let exe = steam::fake_exe_path(&steam_dir, &info.installdir, &info.executable);
    let acf_path = steam::manifest_path(&steam_dir, 7);
    let s = Settings { chosen_folder: root.path().to_path_buf(), auto_delete: true, timer_minutes: 15, fake_exe_dir: "Win64".into() };

    let starter: Starter = {
        let (exe, steam_dir, info, s, source) = (exe.clone(), steam_dir.clone(), info.clone(), s.clone(), source.clone());
        Box::new(move || {
            let acf = steam::write_appmanifest(7, &info, &steam_dir)?;
            fake::create_fake(&source, &exe, &fake::baked_for(&s, Some(&acf)))?;
            let limit = fake::limit_dir_for(&exe, &s.chosen_folder);
            Ok(NewSession { exe_path: exe.clone(), started_unix: 0, child: None, files: vec![exe.clone(), acf], dirs: fake::dirs_to_clean(&exe, &limit) })
        })
    };

    let mut sessions = Sessions::default();
    let id = sessions.enqueue(info.name.clone(), Kind::Steam, 15, true, exe.to_string_lossy().into_owned(), starter).unwrap();
    assert!(!acf_path.exists() && !exe.exists(), "nothing is written while the game only waits");

    let now = now_unix();
    sessions.tick(now);
    assert_eq!(sessions.list(now)[0].state, State::Running);
    assert!(steam::is_our_manifest(&std::fs::read_to_string(&acf_path).unwrap()));
    let cfg = baked::read_from_exe(&exe).unwrap();
    assert_eq!(cfg.steam_manifest_path.as_deref().map(|p| p.ends_with("appmanifest_7.acf")), Some(true));

    sessions.stop(id).unwrap();
    assert!(!exe.exists());
    assert!(!acf_path.exists());
    assert!(!steam_dir.join("steamapps/common/Cool Game").exists(), "game folder removed");
    assert!(steam_dir.join("steamapps/common").exists(), "cleanup stops at steamapps/common");
}

#[test]
fn a_real_game_in_the_way_is_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let steam_dir = root.path().join("Steam");
    let info = info();
    let exe = steam::fake_exe_path(&steam_dir, &info.installdir, &info.executable);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"REAL GAME").unwrap();
    let acf = steam::manifest_path(&steam_dir, 7);
    std::fs::create_dir_all(acf.parent().unwrap()).unwrap();
    std::fs::write(&acf, "\"AppState\" { \"StateFlags\" \"4\" \"SizeOnDisk\" \"123\" }").unwrap();

    assert!(fake::ensure_safe_to_write(&exe).is_err());
    assert!(steam::ensure_manifest_safe(&acf).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"REAL GAME");
}

#[test]
fn a_game_that_turns_out_blocked_at_its_turn_fails_without_writing_a_manifest() {
    let root = tempfile::tempdir().unwrap();
    let steam_dir = root.path().join("Steam");
    std::fs::create_dir_all(steam_dir.join("steamapps")).unwrap();
    let source = root.path().join("source.exe");
    std::fs::write(&source, b"orb").unwrap();
    let info = info();
    let exe = steam::fake_exe_path(&steam_dir, &info.installdir, &info.executable);
    let acf_path = steam::manifest_path(&steam_dir, 7);
    let s = Settings { chosen_folder: root.path().to_path_buf(), ..Settings::default() };

    let starter: Starter = {
        let (exe, steam_dir, info, source, s) = (exe.clone(), steam_dir.clone(), info.clone(), source.clone(), s.clone());
        Box::new(move || {
            let acf = steam::write_appmanifest(7, &info, &steam_dir)?;
            match fake::create_fake(&source, &exe, &fake::baked_for(&s, Some(&acf))) {
                Ok(()) => Ok(NewSession { exe_path: exe, started_unix: 0, child: None, files: vec![], dirs: vec![] }),
                Err(e) => {
                    let _ = std::fs::remove_file(&acf);
                    Err(e)
                }
            }
        })
    };
    let mut sessions = Sessions::default();
    sessions.enqueue("Cool Game".into(), Kind::Steam, 15, false, "cool".into(), starter).unwrap();
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"REAL GAME").unwrap();
    sessions.tick(now_unix());

    let item = &sessions.list(now_unix())[0];
    assert_eq!(item.state, State::Failed);
    assert!(item.error.as_deref().unwrap().contains("not an orbshacker fake"));
    assert_eq!(std::fs::read(&exe).unwrap(), b"REAL GAME");
    assert!(!acf_path.exists(), "the manifest written first was rolled back");
    let _: PathBuf = exe;
}
