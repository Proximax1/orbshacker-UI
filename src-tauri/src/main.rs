#![cfg_attr(windows, windows_subsystem = "windows")]


use orbshacker_core::baked;
use orbshacker_core::discord_db::DiscordDb;
use orbshacker_core::error::{Error, Result};
use orbshacker_core::fake;
use orbshacker_core::http::UreqHttp;
use orbshacker_core::paths::{sanitize_filename, sanitize_path_segment, sanitize_relative_path};
use orbshacker_core::session::{cleanup_paths, Kind, NewSession, SessionInfo, Sessions, Starter};
use orbshacker_core::settings::Settings;
use orbshacker_core::steam::{self, SteamAppInfo, SteamStoreItem};
use orbshacker_core::timer;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

fn mono_now() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs()
}

struct AppState {
    exe: PathBuf,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    db: Mutex<Option<Arc<DiscordDb>>>,
    sessions: Mutex<Sessions>,
    allow_close: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) { s.replace('/', "\\") } else { s.into_owned() }
}


#[derive(Serialize)]
struct Bootstrap {
    version: &'static str,
    developer: &'static str,
    settings: Settings,
    settings_file: String,
    steam_path: Option<String>,
}

#[tauri::command]
async fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap> {
    Ok(Bootstrap {
        version: env!("CARGO_PKG_VERSION"),
        developer: orbshacker_core::DEVELOPER,
        settings: lock(&state.settings).clone(),
        settings_file: state.settings_path.to_string_lossy().into_owned(),
        steam_path: steam::steam_path().map(|p| p.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
async fn settings_save(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    let normalized = settings.normalized()?;
    normalized.save(&state.settings_path)?;
    *lock(&state.settings) = normalized.clone();
    Ok(normalized)
}


#[derive(Serialize)]
struct DbStatus {
    source: String,
    count: usize,
}

#[tauri::command]
async fn load_database(state: State<'_, AppState>, refresh: bool) -> Result<DbStatus> {
    if !refresh {
        if let Some(db) = lock(&state.db).as_ref() {
            return Ok(DbStatus { source: db.source.clone(), count: db.games.len() });
        }
    }
    let db = Arc::new(DiscordDb::load(&UreqHttp)?);
    let status = DbStatus { source: db.source.clone(), count: db.games.len() };
    *lock(&state.db) = Some(db);
    Ok(status)
}

#[derive(Serialize)]
struct GameView {
    id: String,
    name: String,
    aliases: Vec<String>,
    primary: Option<String>,
    executables: Vec<String>,
}

#[tauri::command]
async fn db_search(state: State<'_, AppState>, query: String) -> Result<Vec<GameView>> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(vec![]);
    }
    let db = lock(&state.db)
        .clone()
        .ok_or_else(|| Error::Invalid("The games database is not loaded yet.".into()))?;
    Ok(db
        .search(&query)
        .into_iter()
        .map(|g| GameView {
            id: g.id().to_string(),
            name: g.name().to_string(),
            aliases: g.aliases().to_vec(),
            primary: g.primary_executable(),
            executables: g.all_executables(),
        })
        .collect())
}

#[tauri::command]
async fn preview_target(state: State<'_, AppState>, exe: String) -> Result<String> {
    let settings = lock(&state.settings).clone();
    Ok(display_path(&fake::target_for(&settings, &exe)))
}


struct SteamJob {
    appid: u64,
    info: SteamAppInfo,
    steam_path: PathBuf,
}

fn make_starter(exe: PathBuf, target: PathBuf, settings: Settings, steam_job: Option<SteamJob>) -> Starter {
    Box::new(move || {
        let manifest = match &steam_job {
            Some(j) => Some(steam::write_appmanifest(j.appid, &j.info, &j.steam_path)?),
            None => None,
        };
        let started = (|| -> Result<NewSession> {
            let cfg = fake::baked_for(&settings, manifest.as_deref());
            fake::create_fake(&exe, &target, &cfg)?;
            let limit = fake::limit_dir_for(&target, &settings.chosen_folder);
            let dirs = fake::dirs_to_clean(&target, &limit);
            let child = match fake::launch(&target) {
                Ok(c) => c,
                Err(e) => {
                    cleanup_paths(std::slice::from_ref(&target), &dirs);
                    return Err(Error::Invalid(format!("Could not start {}: {e}", target.display())));
                }
            };
            let mut files = vec![target.clone()];
            files.extend(manifest.clone());
            Ok(NewSession { exe_path: target.clone(), started_unix: mono_now(), child: Some(child), files, dirs })
        })();
        if started.is_err() {
            if let Some(m) = &manifest {
                let _ = std::fs::remove_file(m);
            }
        }
        started
    })
}

fn info_of(sessions: &mut Sessions, id: u32) -> Result<SessionInfo> {
    sessions
        .list(mono_now())
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| Error::NotFound("The game disappeared from the list.".into()))
}

fn enqueue(state: &AppState, name: String, kind: Kind, settings: &Settings, key: &Path, starter: Starter) -> Result<SessionInfo> {
    let key = key.to_string_lossy().replace('\\', "/").to_lowercase();
    let mut sessions = lock(&state.sessions);
    let id = sessions.enqueue(name, kind, settings.timer_minutes, settings.auto_delete, key, starter)?;
    sessions.tick(mono_now());
    info_of(&mut sessions, id)
}

#[derive(Deserialize)]
struct SpoofReq {
    name: String,
    exe: String,
    kind: String,
}

#[tauri::command]
async fn spoof(state: State<'_, AppState>, req: SpoofReq) -> Result<SessionInfo> {
    if req.exe.trim().is_empty() {
        return Err(Error::Invalid("Enter an executable name.".into()));
    }
    let kind = if req.kind == "discord" { Kind::Discord } else { Kind::Manual };
    let settings = lock(&state.settings).clone();
    let target = fake::target_for(&settings, &req.exe);
    fake::ensure_safe_to_write(&target)?;
    let name = if req.name.trim().is_empty() { req.exe.trim().to_string() } else { req.name };
    let starter = make_starter(state.exe.clone(), target.clone(), settings.clone(), None);
    enqueue(&state, name, kind, &settings, &target, starter)
}


#[tauri::command]
async fn steam_search(query: String) -> Result<Vec<SteamStoreItem>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    steam::search_games(&UreqHttp, query.trim())
}

#[tauri::command]
async fn steam_info(appid: u64) -> Result<SteamAppInfo> {
    steam::fetch_app_info(&UreqHttp, appid)
}

#[derive(Deserialize)]
struct SteamReq {
    appid: u64,
    name: String,
    installdir: String,
    executable: String,
    depot_id: Option<String>,
    steam_path: String,
}

impl SteamReq {
    fn info(&self) -> SteamAppInfo {
        SteamAppInfo {
            name: sanitize_filename(&self.name),
            installdir: sanitize_path_segment(&self.installdir),
            executable: sanitize_relative_path(&self.executable),
            depot_id: self
                .depot_id
                .clone()
                .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())),
        }
    }
}

#[derive(Serialize)]
struct SteamPreview {
    manifest: String,
    exe: String,
    info: SteamAppInfo,
}

#[tauri::command]
async fn steam_preview(req: SteamReq) -> Result<SteamPreview> {
    let info = req.info();
    let sp = Path::new(req.steam_path.trim());
    Ok(SteamPreview {
        manifest: display_path(&steam::manifest_path(sp, req.appid)),
        exe: display_path(&steam::fake_exe_path(sp, &info.installdir, &info.executable)),
        info,
    })
}

#[tauri::command]
async fn steam_spoof(state: State<'_, AppState>, req: SteamReq) -> Result<SessionInfo> {
    let steam_path = PathBuf::from(req.steam_path.trim());
    if !steam_path.is_dir() {
        return Err(Error::Invalid(format!("Steam folder not found: {}", steam_path.display())));
    }
    let info = req.info();
    let settings = lock(&state.settings).clone();
    let exe_path = steam::fake_exe_path(&steam_path, &info.installdir, &info.executable);

    steam::ensure_manifest_safe(&steam::manifest_path(&steam_path, req.appid))?;
    fake::ensure_safe_to_write(&exe_path)?;

    let name = info.name.clone();
    let starter = make_starter(state.exe.clone(), exe_path.clone(), settings.clone(), Some(SteamJob { appid: req.appid, info, steam_path }));
    enqueue(&state, name, Kind::Steam, &settings, &exe_path, starter)
}


#[tauri::command]
async fn sessions_list(state: State<'_, AppState>) -> Result<Vec<SessionInfo>> {
    let mut s = lock(&state.sessions);
    s.tick(mono_now());
    Ok(s.list(mono_now()))
}

#[derive(Serialize)]
struct Outcome {
    failures: Vec<String>,
    deleted: bool,
}

#[tauri::command]
async fn session_stop(state: State<'_, AppState>, id: u32) -> Result<Outcome> {
    let mut s = lock(&state.sessions);
    let (failures, deleted) = s.stop(id)?;
    s.tick(mono_now());
    Ok(Outcome { failures, deleted })
}

#[tauri::command]
async fn session_cancel(state: State<'_, AppState>, id: u32) -> Result<()> {
    lock(&state.sessions).cancel(id)
}

#[tauri::command]
async fn session_clean(state: State<'_, AppState>, id: u32) -> Result<Outcome> {
    Ok(Outcome { failures: lock(&state.sessions).clean(id)?, deleted: true })
}

#[tauri::command]
async fn quit_app(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    lock(&state.sessions).stop_all_and_close();
    state.allow_close.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn session_dismiss(state: State<'_, AppState>, id: u32) -> Result<()> {
    lock(&state.sessions).dismiss(id)
}


fn main() {
    let exe = std::env::current_exe().expect("cannot locate own executable");

    if let Some(cfg) = baked::read_from_exe(&exe) {
        timer::run_fake_game(&cfg, &exe);
        return;
    }

    let settings_path = exe.parent().unwrap_or_else(|| Path::new(".")).join("settings.json");
    let state = AppState {
        exe,
        settings: Mutex::new(Settings::load(&settings_path)),
        settings_path,
        db: Mutex::new(None),
        sessions: Mutex::new(Sessions::default()),
        allow_close: AtomicBool::new(false),
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let st = window.state::<AppState>();
                if !st.allow_close.load(Ordering::SeqCst) && lock(&st.sessions).pending_count() > 0 {
                    api.prevent_close();
                    let _ = window.emit("close-requested", ());
                }
            }
        })
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("orbshacker")
                .inner_size(1120.0, 720.0)
                .min_inner_size(900.0, 600.0)
                .decorations(false)
                .theme(Some(tauri::Theme::Dark))
                .background_color(tauri::window::Color(17, 17, 17, 255))
                .center()
                .build()?;

            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(500));
                lock(&handle.state::<AppState>().sessions).tick(mono_now());
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            settings_save,
            load_database,
            db_search,
            preview_target,
            spoof,
            steam_search,
            steam_info,
            steam_preview,
            steam_spoof,
            sessions_list,
            session_stop,
            session_cancel,
            quit_app,
            session_clean,
            session_dismiss,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the app");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                let _ = lock(&state.sessions).shutdown();
            }
        }
    });
}
