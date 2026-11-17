use orbshacker_core::baked;
use orbshacker_core::fake;
use orbshacker_core::session::{now_unix, Kind, NewSession, Sessions, Starter, State};
use orbshacker_core::settings::Settings;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn real_exe() -> PathBuf {
    let p = PathBuf::from(std::env::var("ORBSHACKER_EXE").expect("set ORBSHACKER_EXE to the built orbshacker.exe"));
    assert!(p.exists(), "{} not found", p.display());
    p
}

fn wait_until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn settings_in(dir: &Path, auto_delete: bool, minutes: u32) -> Settings {
    Settings { chosen_folder: dir.to_path_buf(), auto_delete, timer_minutes: minutes, fake_exe_dir: "Win64".into() }
}

fn real_starter(s: &Settings, name: &str) -> (Starter, PathBuf) {
    let target = fake::target_for(s, name);
    let (exe, t, s) = (real_exe(), target.clone(), s.clone());
    let starter: Starter = Box::new(move || {
        fake::create_fake(&exe, &t, &fake::baked_for(&s, None))?;
        let child = fake::launch(&t)?;
        let limit = fake::limit_dir_for(&t, &s.chosen_folder);
        Ok(NewSession { exe_path: t.clone(), started_unix: 0, child: Some(child), files: vec![t.clone()], dirs: fake::dirs_to_clean(&t, &limit) })
    });
    (starter, target)
}

#[test]
#[ignore]
fn start_then_stop_with_auto_delete_cleans_everything() {
    let dir = tempfile::tempdir().unwrap();
    let s = settings_in(dir.path(), true, 15);
    let (starter, target) = real_starter(&s, "Test Game");
    let mut sessions = Sessions::default();
    let id = sessions.enqueue("Test Game".into(), Kind::Manual, 15, true, "t".into(), starter).unwrap();
    sessions.tick(now_unix());
    assert!(baked::read_from_exe(&target).is_some(), "config is baked into the copy");
    std::thread::sleep(Duration::from_secs(3));
    sessions.tick(now_unix());
    assert_eq!(sessions.list(now_unix())[0].state, State::Running, "the headless fake is alive after 3s");
    assert!(sessions.stop(id).unwrap().0.is_empty(), "nothing is locked after kill");
    assert!(!target.exists(), "exe deleted");
    assert!(!target.parent().unwrap().exists(), "empty Win64 folder removed");
    assert!(dir.path().exists(), "the chosen folder itself stays");
}

#[test]
#[ignore]
fn a_fake_ends_and_deletes_itself_when_its_own_timer_runs_out() {
    let dir = tempfile::tempdir().unwrap();
    let s = settings_in(dir.path(), true, 0);
    let target = fake::target_for(&s, "Self Destruct");
    fake::create_fake(&real_exe(), &target, &fake::baked_for(&s, None)).unwrap();
    let mut child = fake::launch(&target).unwrap();
    assert!(wait_until(20, || child.try_wait().ok().flatten().is_some()), "process exited by itself");
    assert!(wait_until(20, || !target.exists()), "exe removed by the self-destruct command");
    assert!(wait_until(10, || !target.parent().unwrap().exists()), "empty folder removed");
}

#[test]
#[ignore]
fn a_fake_without_auto_delete_ends_but_keeps_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let s = settings_in(dir.path(), false, 0);
    let target = fake::target_for(&s, "Stays Put");
    fake::create_fake(&real_exe(), &target, &fake::baked_for(&s, None)).unwrap();
    let mut child = fake::launch(&target).unwrap();
    assert!(wait_until(20, || child.try_wait().ok().flatten().is_some()), "process exited by itself");
    std::thread::sleep(Duration::from_secs(4));
    assert!(target.exists(), "files kept");
}

#[test]
#[ignore]
fn two_real_games_run_one_after_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let s = settings_in(dir.path(), true, 15);
    let (sa, a) = real_starter(&s, "Game A");
    let (sb, b) = real_starter(&s, "Game B");
    let mut sessions = Sessions::default();
    sessions.enqueue("A".into(), Kind::Manual, 0, true, "a".into(), sa).unwrap();
    sessions.enqueue("B".into(), Kind::Manual, 0, true, "b".into(), sb).unwrap();

    sessions.tick(now_unix());
    assert!(a.exists() && !b.exists(), "only A exists during A's turn");
    assert_eq!(sessions.list(now_unix())[1].state, State::Queued);

    sessions.tick(now_unix() + 1);
    assert!(!a.exists(), "A cleaned up");
    assert!(b.exists(), "B started");
    sessions.tick(now_unix() + 2);
    let states: Vec<State> = sessions.list(now_unix()).iter().map(|i| i.state).collect();
    assert_eq!(states, vec![State::Done, State::Done]);
    assert!(!b.exists(), "B cleaned up");
}
