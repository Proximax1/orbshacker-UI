use crate::error::{Error, Result};
use crate::timer::remaining_secs;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Child;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Discord,
    Manual,
    Steam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Queued,
    Running,
    Done,
    Stopped,
    Exited,
    Failed,
}

pub struct NewSession {
    pub exe_path: PathBuf,
    pub started_unix: u64,
    pub child: Option<Child>,
    pub files: Vec<PathBuf>,
    pub dirs: Vec<PathBuf>,
}

pub type Starter = Box<dyn FnOnce() -> Result<NewSession> + Send>;

enum Status {
    Queued(Option<Starter>),
    Live,
    Done,
    Stopped,
    Exited,
    Failed,
}

struct Item {
    id: u32,
    key: String,
    name: String,
    kind: Kind,
    minutes: u32,
    auto_delete: bool,
    status: Status,
    live: Option<NewSession>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub kind: Kind,
    pub exe_path: String,
    pub minutes: u32,
    pub auto_delete: bool,
    pub remaining_secs: u64,
    pub state: State,
    pub queue_pos: Option<usize>,
    pub files_present: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Sessions {
    next_id: u32,
    items: Vec<Item>,
    closed: bool,
}

pub fn cleanup_paths(files: &[PathBuf], dirs: &[PathBuf]) -> Vec<String> {
    let mut failures = Vec::new();
    for f in files {
        let mut deleted = false;
        for _ in 0..5 {
            match std::fs::remove_file(f) {
                Ok(()) => {
                    deleted = true;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    deleted = true;
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
        if !deleted {
            failures.push(format!("{} is locked", f.display()));
        }
    }
    let mut sorted: Vec<&PathBuf> = dirs.iter().collect();
    sorted.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for d in sorted {
        let empty = std::fs::read_dir(d).map(|mut r| r.next().is_none()).unwrap_or(false);
        if empty {
            let _ = std::fs::remove_dir(d);
        }
    }
    failures
}

fn kill(live: &mut NewSession) {
    if let Some(c) = live.child.as_mut() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

impl Sessions {
    pub fn enqueue(&mut self, name: String, kind: Kind, minutes: u32, auto_delete: bool, key: String, starter: Starter) -> Result<u32> {
        if self.closed {
            return Err(Error::Invalid("orbshacker is closing.".into()));
        }
        if self.items.iter().any(|i| i.key == key && matches!(i.status, Status::Live | Status::Queued(_))) {
            return Err(Error::Invalid(format!("{name} is already running or waiting in the queue.")));
        }
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Item {
            id,
            key,
            name,
            kind,
            minutes,
            auto_delete,
            status: Status::Queued(Some(starter)),
            live: None,
            error: None,
        });
        Ok(id)
    }

    fn live_index(&self) -> Option<usize> {
        self.items.iter().position(|i| matches!(i.status, Status::Live))
    }

    pub fn tick(&mut self, now: u64) {
        if self.closed {
            return;
        }
        if let Some(i) = self.live_index() {
            let item = &mut self.items[i];
            let live = item.live.as_mut().expect("a live item has a session");
            let timer_over = remaining_secs(live.started_unix, now, item.minutes) == 0;
            let exited = live.child.as_mut().map(|c| matches!(c.try_wait(), Ok(Some(_)))).unwrap_or(false);
            if timer_over || exited {
                kill(live);
                if item.auto_delete {
                    cleanup_paths(&live.files, &live.dirs);
                }
                item.status = if timer_over { Status::Done } else { Status::Exited };
            }
        }
        while self.live_index().is_none() {
            let Some(next) = self.items.iter().position(|i| matches!(i.status, Status::Queued(_))) else {
                return;
            };
            let item = &mut self.items[next];
            let Status::Queued(starter) = &mut item.status else { return };
            let Some(starter) = starter.take() else { return };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(starter))
                .unwrap_or_else(|_| Err(Error::Invalid("Starting this game failed unexpectedly.".into())));
            match result {
                Ok(mut started) => {
                    started.started_unix = now;
                    item.live = Some(started);
                    item.status = Status::Live;
                }
                Err(e) => {
                    item.error = Some(e.to_string());
                    item.status = Status::Failed;
                }
            }
        }
    }

    pub fn list(&mut self, now: u64) -> Vec<SessionInfo> {
        let mut pos = 0;
        self.items
            .iter()
            .map(|it| {
                let (state, queue_pos) = match it.status {
                    Status::Queued(_) => {
                        pos += 1;
                        (State::Queued, Some(pos))
                    }
                    Status::Live => (State::Running, None),
                    Status::Done => (State::Done, None),
                    Status::Stopped => (State::Stopped, None),
                    Status::Exited => (State::Exited, None),
                    Status::Failed => (State::Failed, None),
                };
                let remaining = match (&it.status, &it.live) {
                    (Status::Live, Some(l)) => remaining_secs(l.started_unix, now, it.minutes),
                    (Status::Queued(_), _) => it.minutes as u64 * 60,
                    _ => 0,
                };
                SessionInfo {
                    id: it.id,
                    name: it.name.clone(),
                    kind: it.kind,
                    exe_path: it.live.as_ref().map(|l| l.exe_path.to_string_lossy().into_owned()).unwrap_or_default(),
                    minutes: it.minutes,
                    auto_delete: it.auto_delete,
                    remaining_secs: remaining,
                    state,
                    queue_pos,
                    files_present: it.live.as_ref().map(|l| l.files.iter().any(|f| f.exists())).unwrap_or(false),
                    error: it.error.clone(),
                }
            })
            .collect()
    }

    fn find(&mut self, id: u32) -> Result<&mut Item> {
        self.items
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NotFound(format!("Game {id} is no longer in the list.")))
    }

    pub fn stop(&mut self, id: u32) -> Result<(Vec<String>, bool)> {
        let item = self.find(id)?;
        if !matches!(item.status, Status::Live) {
            return Err(Error::Invalid(format!("{} is not running any more.", item.name)));
        }
        let live = item.live.as_mut().expect("a live item has a session");
        kill(live);
        item.status = Status::Stopped;
        let deleted = item.auto_delete;
        let failures = if deleted { cleanup_paths(&live.files, &live.dirs) } else { vec![] };
        Ok((failures, deleted))
    }

    pub fn cancel(&mut self, id: u32) -> Result<()> {
        let item = self.find(id)?;
        if !matches!(item.status, Status::Queued(_)) {
            return Err(Error::Invalid(format!("{} has already started. Use Stop to end it.", item.name)));
        }
        self.items.retain(|i| i.id != id);
        Ok(())
    }

    pub fn clean(&mut self, id: u32) -> Result<Vec<String>> {
        let item = self.find(id)?;
        if matches!(item.status, Status::Live) {
            return Err(Error::Invalid("Stop the game before deleting its files.".into()));
        }
        let (files, dirs) = match item.live.as_ref() {
            Some(l) => (l.files.clone(), l.dirs.clone()),
            None => return Ok(vec![]),
        };
        let in_use = self.items.iter().any(|i| {
            i.id != id && matches!(i.status, Status::Live) && i.live.as_ref().is_some_and(|l| l.files.iter().any(|f| files.contains(f)))
        });
        if in_use {
            return Err(Error::Invalid("Another game is using these files right now.".into()));
        }
        Ok(cleanup_paths(&files, &dirs))
    }

    pub fn dismiss(&mut self, id: u32) -> Result<()> {
        let item = self.find(id)?;
        if matches!(item.status, Status::Live | Status::Queued(_)) {
            return Err(Error::Invalid("Stop the game before removing it from the list.".into()));
        }
        if item.live.as_ref().is_some_and(|l| l.files.iter().any(|f| f.exists())) {
            return Err(Error::Invalid(format!("{} still has files on disk. Delete them first.", item.name)));
        }
        self.items.retain(|i| i.id != id);
        Ok(())
    }

    pub fn shutdown(&mut self) -> Vec<String> {
        if self.closed {
            return vec![];
        }
        self.closed = true;
        let mut failures = Vec::new();
        if let Some(i) = self.live_index() {
            let item = &mut self.items[i];
            if item.auto_delete {
                let live = item.live.as_mut().expect("a live item has a session");
                kill(live);
                item.status = Status::Stopped;
                failures.extend(cleanup_paths(&live.files, &live.dirs));
            }
        }
        failures
    }

    pub fn stop_all_and_close(&mut self) -> Vec<String> {
        if self.closed {
            return vec![];
        }
        self.closed = true;
        let mut failures = Vec::new();
        if let Some(i) = self.live_index() {
            let item = &mut self.items[i];
            let live = item.live.as_mut().expect("a live item has a session");
            kill(live);
            item.status = Status::Stopped;
            if item.auto_delete {
                failures.extend(cleanup_paths(&live.files, &live.dirs));
            }
        }
        failures
    }


    pub fn pending_count(&self) -> usize {
        self.items.iter().filter(|i| matches!(i.status, Status::Live | Status::Queued(_))).count()
    }
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn started(files: Vec<PathBuf>, dirs: Vec<PathBuf>) -> NewSession {
        NewSession { exe_path: files.first().cloned().unwrap_or_default(), started_unix: 0, child: None, files, dirs }
    }

    fn ok_starter(log: &Arc<Mutex<Vec<String>>>, name: &str, files: Vec<PathBuf>) -> Starter {
        let log = log.clone();
        let name = name.to_string();
        Box::new(move || {
            log.lock().unwrap().push(name);
            Ok(started(files, vec![]))
        })
    }

    fn add(s: &mut Sessions, name: &str, auto_delete: bool, starter: Starter) -> u32 {
        s.enqueue(name.into(), Kind::Manual, 15, auto_delete, name.to_string(), starter).unwrap()
    }

    fn states(s: &mut Sessions, now: u64) -> Vec<State> {
        s.list(now).iter().map(|i| i.state).collect()
    }

    #[test]
    fn nothing_is_created_until_the_games_turn() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        assert!(log.lock().unwrap().is_empty());
        s.tick(100);
        assert_eq!(*log.lock().unwrap(), vec!["A"]);
    }

    #[test]
    fn only_one_game_is_active_and_the_rest_wait_in_order() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        add(&mut s, "B", false, ok_starter(&log, "B", vec![]));
        add(&mut s, "C", false, ok_starter(&log, "C", vec![]));
        s.tick(100);
        s.tick(101);
        assert_eq!(states(&mut s, 101), vec![State::Running, State::Queued, State::Queued]);
        let pos: Vec<Option<usize>> = s.list(101).iter().map(|i| i.queue_pos).collect();
        assert_eq!(pos, vec![None, Some(1), Some(2)]);
        assert_eq!(*log.lock().unwrap(), vec!["A"]);
    }

    #[test]
    fn next_game_starts_when_the_timer_of_the_active_one_runs_out() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        add(&mut s, "B", false, ok_starter(&log, "B", vec![]));
        s.tick(1000);
        s.tick(1000 + 899);
        assert_eq!(states(&mut s, 1899), vec![State::Running, State::Queued]);
        s.tick(1000 + 900);
        assert_eq!(states(&mut s, 1900), vec![State::Done, State::Running]);
        assert_eq!(*log.lock().unwrap(), vec!["A", "B"]);
        let b = &s.list(1900)[1];
        assert_eq!(b.remaining_secs, 900, "B's countdown starts when its turn starts");
    }

    #[test]
    fn stopping_the_active_game_hands_over_to_the_next() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let a = add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        add(&mut s, "B", false, ok_starter(&log, "B", vec![]));
        s.tick(10);
        s.stop(a).unwrap();
        s.tick(11);
        assert_eq!(states(&mut s, 11), vec![State::Stopped, State::Running]);
    }

    #[test]
    fn a_queued_game_can_be_cancelled_and_leaves_nothing_behind() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        let b = add(&mut s, "B", false, ok_starter(&log, "B", vec![]));
        s.tick(10);
        s.cancel(b).unwrap();
        assert_eq!(states(&mut s, 10), vec![State::Running]);
        s.tick(5000);
        assert_eq!(*log.lock().unwrap(), vec!["A"], "B never started");
    }

    #[test]
    fn a_game_that_fails_to_start_is_marked_failed_and_the_queue_moves_on() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "Bad", false, Box::new(|| Err(Error::Invalid("exists and is not an orbshacker fake".into()))));
        add(&mut s, "Good", false, ok_starter(&log, "Good", vec![]));
        s.tick(10);
        assert_eq!(states(&mut s, 10), vec![State::Failed, State::Running]);
        assert!(s.list(10)[0].error.as_deref().unwrap().contains("not an orbshacker fake"));
    }

    #[test]
    fn timer_end_removes_files_with_auto_delete_and_keeps_them_without() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.exe");
        let b = root.path().join("b.exe");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", true, ok_starter(&log, "A", vec![a.clone()]));
        add(&mut s, "B", false, ok_starter(&log, "B", vec![b.clone()]));
        s.tick(0);
        s.tick(900);
        assert!(!a.exists(), "auto-delete cleaned A");
        s.tick(901);
        s.tick(901 + 900);
        assert!(b.exists(), "B kept its files");
        assert_eq!(states(&mut s, 2000), vec![State::Done, State::Done]);
    }

    #[test]
    fn stop_keeps_files_without_auto_delete_and_clean_removes_them() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("g.exe");
        std::fs::write(&exe, b"x").unwrap();
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let id = add(&mut s, "A", false, ok_starter(&log, "A", vec![exe.clone()]));
        s.tick(1);
        s.stop(id).unwrap();
        assert!(exe.exists());
        assert!(s.list(1)[0].files_present);
        s.clean(id).unwrap();
        assert!(!exe.exists());
    }

    #[test]
    fn stop_deletes_files_with_auto_delete() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("g.exe");
        std::fs::write(&exe, b"x").unwrap();
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let id = add(&mut s, "A", true, ok_starter(&log, "A", vec![exe.clone()]));
        s.tick(1);
        s.stop(id).unwrap();
        assert!(!exe.exists());
    }

    #[test]
    fn clean_and_dismiss_refuse_while_active_or_queued() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let a = add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        let b = add(&mut s, "B", false, ok_starter(&log, "B", vec![]));
        s.tick(1);
        assert!(s.clean(a).is_err());
        assert!(s.dismiss(a).is_err());
        assert!(s.dismiss(b).is_err());
        s.stop(a).unwrap();
        s.dismiss(a).unwrap();
        assert_eq!(s.list(2).len(), 1);
    }

    #[test]
    fn the_same_target_cannot_be_queued_twice() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        let again = s.enqueue("A".into(), Kind::Manual, 15, false, "A".into(), ok_starter(&log, "A", vec![]));
        assert!(matches!(again, Err(Error::Invalid(_))));
        s.tick(0);
        s.tick(900);
        assert!(s.enqueue("A".into(), Kind::Manual, 15, false, "A".into(), ok_starter(&log, "A", vec![])).is_ok());
    }

    #[test]
    fn cancel_refuses_a_game_that_already_started() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let a = add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        s.tick(1);
        assert!(s.cancel(a).is_err());
        assert_eq!(states(&mut s, 1), vec![State::Running], "it was not touched");
    }

    #[test]
    fn stop_refuses_a_game_that_is_not_running() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let a = add(&mut s, "A", true, ok_starter(&log, "A", vec![]));
        s.tick(0);
        s.tick(900);
        assert!(s.stop(a).is_err());
    }

    #[test]
    fn stop_reports_whether_files_were_deleted() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let a = add(&mut s, "A", true, ok_starter(&log, "A", vec![]));
        let b = add(&mut s, "B", false, ok_starter(&log, "B", vec![]));
        s.tick(0);
        assert!(s.stop(a).unwrap().1);
        s.tick(1);
        assert!(!s.stop(b).unwrap().1);
    }

    #[test]
    fn nothing_starts_or_ends_after_the_app_closed() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", false, ok_starter(&log, "A", vec![]));
        add(&mut s, "B", false, ok_starter(&log, "B", vec![]));
        s.tick(0);
        s.stop_all_and_close();
        s.tick(5000);
        assert_eq!(*log.lock().unwrap(), vec!["A"], "B never started after closing");
        assert!(s.enqueue("C".into(), Kind::Manual, 15, false, "C".into(), ok_starter(&log, "C", vec![])).is_err());
    }

    #[test]
    fn a_card_with_files_on_disk_cannot_be_dismissed() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.exe");
        std::fs::write(&a, b"x").unwrap();
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let id = add(&mut s, "A", false, ok_starter(&log, "A", vec![a.clone()]));
        s.tick(0);
        s.stop(id).unwrap();
        assert!(s.dismiss(id).is_err(), "files are still there");
        assert!(a.exists());
        s.clean(id).unwrap();
        assert!(s.dismiss(id).is_ok());
    }

    #[test]
    fn a_starter_that_panics_fails_the_game_and_the_queue_moves_on() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "Boom", false, Box::new(|| panic!("boom")));
        add(&mut s, "Fine", false, ok_starter(&log, "Fine", vec![]));
        s.tick(0);
        assert_eq!(states(&mut s, 0), vec![State::Failed, State::Running]);
    }

    #[test]
    fn deleting_the_files_of_an_old_game_spares_a_newer_game_using_them() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("g.exe");
        std::fs::write(&exe, b"x").unwrap();
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        let old = add(&mut s, "G", false, ok_starter(&log, "G", vec![exe.clone()]));
        s.tick(0);
        s.stop(old).unwrap();
        s.enqueue("G".into(), Kind::Manual, 15, false, "G".into(), ok_starter(&log, "G2", vec![exe.clone()])).unwrap();
        s.tick(1);
        assert!(s.clean(old).is_err());
        assert!(exe.exists());
    }

    #[test]
    fn unknown_game_is_a_clear_error() {
        let mut s = Sessions::default();
        assert!(matches!(s.stop(99), Err(Error::NotFound(_))));
    }

    #[test]
    fn shutdown_cleans_only_an_active_auto_delete_game() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.exe");
        std::fs::write(&a, b"x").unwrap();
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        add(&mut s, "A", true, ok_starter(&log, "A", vec![a.clone()]));
        add(&mut s, "B", true, ok_starter(&log, "B", vec![]));
        s.tick(1);
        s.shutdown();
        assert!(!a.exists());
        assert_eq!(s.pending_count(), 1, "the active game ended, the waiting one was not touched");
    }

    #[test]
    fn cleanup_removes_files_and_only_empty_dirs() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("Win64").join("Bin");
        std::fs::create_dir_all(&inner).unwrap();
        let exe = inner.join("g.exe");
        std::fs::write(&exe, b"x").unwrap();
        let keep = root.path().join("Win64").join("keep.txt");
        std::fs::write(&keep, b"user data").unwrap();
        let fails = cleanup_paths(std::slice::from_ref(&exe), &[inner.clone(), root.path().join("Win64")]);
        assert!(fails.is_empty());
        assert!(!exe.exists());
        assert!(!inner.exists(), "empty inner dir removed");
        assert!(keep.exists(), "folder with user data stays");
    }

    #[test]
    fn cleanup_of_missing_file_is_not_a_failure() {
        let root = tempfile::tempdir().unwrap();
        assert!(cleanup_paths(&[root.path().join("gone.exe")], &[]).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn a_process_that_dies_early_is_exited_and_the_queue_moves_on() {
        let log = Arc::new(Mutex::new(vec![]));
        let mut s = Sessions::default();
        s.enqueue(
            "Short".into(),
            Kind::Manual,
            15,
            false,
            "Short".into(),
            Box::new(|| {
                let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn()?;
                Ok(NewSession { exe_path: PathBuf::new(), started_unix: 0, child: Some(child), files: vec![], dirs: vec![] })
            }),
        )
        .unwrap();
        add(&mut s, "Next", false, ok_starter(&log, "Next", vec![]));
        s.tick(10);
        std::thread::sleep(Duration::from_millis(800));
        s.tick(11);
        assert_eq!(states(&mut s, 11), vec![State::Exited, State::Running]);
    }
}
