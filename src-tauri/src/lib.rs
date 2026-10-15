pub mod engine;
pub mod fingerprint;
pub mod hints;
pub mod history;
pub mod models;
pub mod ops;
pub mod settings;
pub mod sources;
pub mod tags;

use futures::StreamExt;
use history::History;
use models::*;
use serde::Serialize;
use settings::Settings;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    tracks: Mutex<HashMap<String, Track>>,
    order: Mutex<Vec<String>>,
    proposals: Mutex<HashMap<String, Proposal>>,
    settings: RwLock<Settings>,
    data_dir: PathBuf,
    history: History,
    cancel: AtomicBool,
    busy: AtomicBool,
    thumbs: Mutex<HashMap<String, String>>,
    apply_queue: Mutex<std::collections::VecDeque<String>>,
    apply_running: AtomicBool,
    apply_total: AtomicUsize,
    apply_done: AtomicUsize,
    apply_errors: AtomicUsize,
}

type St<'a> = State<'a, Arc<AppState>>;

impl AppState {
    fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }
    fn track(&self, id: &str) -> Result<Track, String> {
        self.tracks.lock().unwrap().get(id).cloned().ok_or_else(|| "Nie znaleziono utworu".to_string())
    }
    fn put(&self, t: Track) {
        self.thumbs.lock().unwrap().remove(&t.id);
        self.tracks.lock().unwrap().insert(t.id.clone(), t);
    }
    fn set_status(&self, id: &str, st: TrackStatus) -> Option<Track> {
        let mut tr = self.tracks.lock().unwrap();
        let t = tr.get_mut(id)?;
        t.status = st;
        Some(t.clone())
    }
    fn replace(&self, old_id: &str, t: Track) {
        let mut order = self.order.lock().unwrap();
        if let Some(pos) = order.iter().position(|x| x == old_id) {
            order[pos] = t.id.clone();
        }
        self.tracks.lock().unwrap().remove(old_id);
        self.proposals.lock().unwrap().remove(old_id);
        self.thumbs.lock().unwrap().remove(old_id);
        self.put(t);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct JobProgress {
    kind: String,
    done: usize,
    total: usize,
    running: bool,
    errors: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Replaced {
    old_id: String,
    track: Track,
}

fn emit_track(app: &AppHandle, t: &Track, p: Option<Proposal>) {
    let _ = app.emit("track-update", TrackEvent { track: t.clone(), proposal: p });
}

// ───────────────────────────── Ustawienia / info ─────────────────────────────

#[tauri::command]
fn get_settings(state: St) -> Settings {
    state.settings()
}

#[tauri::command]
fn save_settings(state: St, settings: Settings) -> Result<Settings, String> {
    let s = settings.sanitized();
    s.save(&state.data_dir)?;
    *state.settings.write().unwrap() = s.clone();
    // Ocena kompletności zależy od min. rozmiaru okładki.
    let mut tr = state.tracks.lock().unwrap();
    for t in tr.values_mut() {
        tags::evaluate(t, s.min_cover_size);
    }
    Ok(s)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: String,
    platform: String,
    data_dir: String,
}

#[tauri::command]
fn app_info(state: St) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        data_dir: state.data_dir.to_string_lossy().into(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    available: bool,
    latest: String,
    url: String,
    download: Option<String>,
    notes: String,
}

fn version_tuple(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split(['.', '-']).filter_map(|x| x.parse().ok()).collect()
}

#[tauri::command]
async fn check_update() -> Result<UpdateInfo, String> {
    let url = "https://api.github.com/repos/BlackSingularities/MusicTagger/releases/latest";
    let v: serde_json::Value = sources::http::get_json("github", url).await.map_err(|e| e.to_string())?;
    let tag = v.get("tag_name").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let want = if cfg!(target_os = "android") { ".apk" } else if cfg!(windows) { "setup.exe" } else { ".AppImage" };
    let download = v.get("assets").and_then(|a| a.as_array()).and_then(|a| {
        a.iter()
            .find(|x| x.get("name").and_then(|n| n.as_str()).map(|n| n.ends_with(want)).unwrap_or(false))
            .and_then(|x| x.get("browser_download_url").and_then(|u| u.as_str()).map(String::from))
    });
    Ok(UpdateInfo {
        available: version_tuple(&tag) > version_tuple(env!("CARGO_PKG_VERSION")),
        latest: tag,
        url: v.get("html_url").and_then(|x| x.as_str()).unwrap_or("").into(),
        download,
        notes: v.get("body").and_then(|x| x.as_str()).unwrap_or("").chars().take(2000).collect(),
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    received: u64,
    total: u64,
    done: bool,
}

fn safe_file_name(url: &str) -> String {
    let raw = url.rsplit('/').next().unwrap_or("MusicTagger-update");
    let name: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    if name.len() < 3 {
        "MusicTagger-update".into()
    } else {
        name
    }
}

/// Pobiera plik aktualizacji do katalogu danych; po drodze wysyła zdarzenia `update-progress`.
#[tauri::command]
async fn download_update(app: AppHandle, url: String) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;

    if !url.starts_with("https://") {
        return Err("Nieprawidłowy adres aktualizacji".into());
    }
    let dir = {
        let st = app.state::<Arc<AppState>>();
        st.data_dir.join("updates")
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = safe_file_name(&url);
    let path = dir.join(&name);
    // Zostaje tylko aktualnie pobierany plik – starsze wersje zajmują miejsce niepotrzebnie.
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.file_name() != std::ffi::OsStr::new(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    let resp = sources::http::CLIENT
        .get(&url)
        .timeout(std::time::Duration::from_secs(3600))
        .send()
        .await
        .map_err(|e| format!("Nie udało się pobrać aktualizacji: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Nie udało się pobrać aktualizacji (HTTP {})", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    // Jeśli ten sam plik jest już pobrany w całości, nie pobieramy go drugi raz.
    if total > 0 {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() == total {
                let _ = app.emit("update-progress", UpdateProgress { received: total, total, done: true });
                return Ok(path.to_string_lossy().to_string());
            }
        }
    }

    let part = dir.join(format!("{name}.part"));
    let mut file = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
    let mut resp = resp;
    let mut received: u64 = 0;
    let mut last = std::time::Instant::now();
    let _ = app.emit("update-progress", UpdateProgress { received: 0, total, done: false });
    loop {
        let chunk = resp.chunk().await.map_err(|e| format!("Przerwane pobieranie: {e}"))?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        if last.elapsed() >= std::time::Duration::from_millis(200) {
            last = std::time::Instant::now();
            let _ = app.emit("update-progress", UpdateProgress { received, total, done: false });
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if total > 0 && received != total {
        let _ = std::fs::remove_file(&part);
        return Err("Pobrany plik jest niekompletny".into());
    }
    let _ = std::fs::remove_file(&path);
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    let _ = app.emit("update-progress", UpdateProgress { received, total: received, done: true });
    Ok(path.to_string_lossy().to_string())
}

/// Uruchamia pobrany instalator. Windows: instalacja w tle i ponowne uruchomienie
/// aplikacji; Android: systemowy instalator pakietu (wymaga potwierdzenia).
#[tauri::command]
async fn install_update(app: AppHandle, path: String) -> Result<(), String> {
    if !Path::new(&path).is_file() {
        return Err("Nie znaleziono pobranego pliku aktualizacji".into());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        // /UPDATE – tryb aktualizacji (bez pytań o katalog), /P – pasywny, /R – restart aplikacji.
        std::process::Command::new(&path)
            .args(["/UPDATE", "/P", "/R"])
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map_err(|e| format!("Nie udało się uruchomić instalatora: {e}"))?;
        let h = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            h.exit(0);
        });
        Ok(())
    }
    #[cfg(target_os = "android")]
    {
        let _ = &app;
        // Odbiera to MainActivity i uruchamia systemowy instalator pakietu.
        let uri = format!("musictagger://install?path={}", urlencoding::encode(&path));
        tauri_plugin_opener::open_url(uri, None::<&str>).map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "windows", target_os = "android")))]
    {
        let _ = &app;
        tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
    }
}

// ───────────────────────────── Przeglądanie folderów ─────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirListing {
    path: String,
    parent: Option<String>,
    dirs: Vec<DirEntry>,
    audio_here: usize,
}

#[tauri::command]
fn get_roots() -> Vec<DirEntry> {
    let mut out = vec![];
    let mut push = |name: &str, p: PathBuf| {
        if p.is_dir() && !out.iter().any(|d: &DirEntry| d.path == p.to_string_lossy()) {
            out.push(DirEntry { name: name.into(), path: p.to_string_lossy().into(), audio_count: 0 });
        }
    };
    if cfg!(target_os = "android") {
        let base = PathBuf::from("/storage/emulated/0");
        push("Music", base.join("Music"));
        push("Download", base.join("Download"));
        push("Pamięć wewnętrzna", base.clone());
        if let Ok(rd) = std::fs::read_dir("/storage") {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n != "emulated" && n != "self" {
                    push(&format!("Karta SD ({n})"), e.path());
                }
            }
        }
    } else if cfg!(windows) {
        if let Ok(home) = std::env::var("USERPROFILE") {
            let h = PathBuf::from(home);
            push("Muzyka", h.join("Music"));
            push("Pobrane", h.join("Downloads"));
            push("Pulpit", h.join("Desktop"));
        }
        for c in b'C'..=b'Z' {
            let d = format!("{}:\\", c as char);
            push(&d, PathBuf::from(&d));
        }
    } else if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        push("Music", h.join("Music"));
        push("Downloads", h.join("Downloads"));
        push("Home", h);
        push("/", PathBuf::from("/"));
    }
    out
}

#[tauri::command]
fn list_dirs(path: String) -> Result<DirListing, String> {
    let p = PathBuf::from(&path);
    let rd = std::fs::read_dir(&p).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "Brak uprawnień do tego folderu".to_string()
        } else {
            e.to_string()
        }
    })?;
    let mut dirs = vec![];
    let mut audio = 0;
    for e in rd.flatten() {
        let ep = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        if ep.is_dir() {
            dirs.push(DirEntry { name, path: ep.to_string_lossy().into(), audio_count: 0 });
        } else if tags::is_audio(&ep) {
            audio += 1;
        }
    }
    dirs.sort_by_key(|d| d.name.to_lowercase());
    Ok(DirListing {
        parent: p.parent().map(|x| x.to_string_lossy().to_string()),
        path,
        dirs,
        audio_here: audio,
    })
}

#[tauri::command]
fn storage_access() -> bool {
    if cfg!(target_os = "android") {
        std::fs::read_dir("/storage/emulated/0").map(|mut r| r.next().is_some()).unwrap_or(false)
    } else {
        true
    }
}

// ───────────────────────────── Skanowanie ─────────────────────────────

/// Czyta tagi z listy plików równolegle (z postępem) i ustawia/uzupełnia bibliotekę.
async fn load_files(app: AppHandle, state: Arc<AppState>, files: Vec<PathBuf>, append: bool) -> Result<Vec<Track>, String> {
    let min_cover = state.settings().min_cover_size;
    let app2 = app.clone();
    let st2 = state.clone();
    let mut tracks = tauri::async_runtime::spawn_blocking(move || {
        let total = files.len();
        let done = AtomicUsize::new(0);
        let results: Mutex<Vec<Track>> = Mutex::new(Vec::with_capacity(total));
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
        let next = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| loop {
                    if st2.cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(f) = files.get(i) else { break };
                    let mut t = tags::read_track(f);
                    tags::evaluate(&mut t, min_cover);
                    results.lock().unwrap().push(t);
                    let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                    if d % 15 == 0 || d == total {
                        let _ = app2.emit(
                            "scan-progress",
                            ScanProgress { done: d, total, current: f.to_string_lossy().into(), phase: "read".into() },
                        );
                    }
                });
            }
        });
        results.into_inner().unwrap()
    })
    .await
    .map_err(|e| e.to_string())?;

    if append {
        let tr = state.tracks.lock().unwrap();
        tracks.retain(|t| !tr.contains_key(&t.id));
        tracks.extend(tr.values().cloned());
    }
    tracks.sort_by(|a, b| {
        (a.folder.to_lowercase(), a.tags.disc.unwrap_or(1), a.tags.track.unwrap_or(999), a.file_name.to_lowercase())
            .cmp(&(b.folder.to_lowercase(), b.tags.disc.unwrap_or(1), b.tags.track.unwrap_or(999), b.file_name.to_lowercase()))
    });
    if append {
        // Zachowujemy statusy i propozycje już wczytanych utworów.
        let tr = state.tracks.lock().unwrap();
        for t in tracks.iter_mut() {
            if let Some(old) = tr.get(&t.id) {
                *t = old.clone();
            }
        }
    }
    *state.order.lock().unwrap() = tracks.iter().map(|t| t.id.clone()).collect();
    *state.tracks.lock().unwrap() = tracks.iter().map(|t| (t.id.clone(), t.clone())).collect();
    if !append {
        state.proposals.lock().unwrap().clear();
        state.thumbs.lock().unwrap().clear();
    }
    let _ = app.emit("scan-progress", ScanProgress { done: tracks.len(), total: tracks.len(), current: String::new(), phase: "done".into() });
    Ok(tracks)
}

#[tauri::command]
async fn scan_folder(
    app: AppHandle,
    state: St<'_>,
    path: String,
    recursive: Option<bool>,
    append: Option<bool>,
) -> Result<Vec<Track>, String> {
    let state = state.inner().clone();
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err("Folder nie istnieje lub brak dostępu".into());
    }
    let recursive = recursive.unwrap_or(state.settings().recursive);
    {
        let mut s = state.settings.write().unwrap();
        s.recent_folders.retain(|f| f != &path);
        s.recent_folders.insert(0, path.clone());
        s.recent_folders.truncate(8);
        s.recursive = recursive;
        let _ = s.save(&state.data_dir);
    }
    state.cancel.store(false, Ordering::SeqCst);
    let _ = app.emit("scan-progress", ScanProgress { done: 0, total: 0, current: path.clone(), phase: "collect".into() });
    let files = tauri::async_runtime::spawn_blocking(move || {
        walkdir::WalkDir::new(&root)
            .follow_links(false)
            .max_depth(if recursive { 64 } else { 1 })
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
            .flatten()
            .filter(|e| e.file_type().is_file() && tags::is_audio(e.path()))
            .map(|e| e.into_path())
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| e.to_string())?;
    load_files(app, state, files, append.unwrap_or(false)).await
}

/// Wczytuje wybrane pliki (bez przeszukiwania folderów).
#[tauri::command]
async fn scan_files(app: AppHandle, state: St<'_>, paths: Vec<String>, append: Option<bool>) -> Result<Vec<Track>, String> {
    let state = state.inner().clone();
    state.cancel.store(false, Ordering::SeqCst);
    let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).filter(|p| p.is_file() && tags::is_audio(p)).collect();
    if files.is_empty() {
        return Err("Nie wybrano obsługiwanych plików audio".into());
    }
    load_files(app, state, files, append.unwrap_or(false)).await
}

#[tauri::command]
fn reload_track(state: St, id: String) -> Result<Track, String> {
    let t = state.track(&id)?;
    let mut n = tags::read_track(Path::new(&t.path));
    tags::evaluate(&mut n, state.settings().min_cover_size);
    n.status = t.status;
    state.put(n.clone());
    Ok(n)
}

// ───────────────────────────── Okładki (podgląd) ─────────────────────────────

#[tauri::command]
async fn get_cover(state: St<'_>, id: String, size: Option<u32>) -> Result<Option<String>, String> {
    let size = size.unwrap_or(96);
    let key = format!("{id}:{size}");
    if let Some(c) = state.thumbs.lock().unwrap().get(&key) {
        return Ok(Some(c.clone()));
    }
    let t = state.track(&id)?;
    if t.cover.is_none() {
        return Ok(None);
    }
    let path = PathBuf::from(&t.path);
    let url = tauri::async_runtime::spawn_blocking(move || {
        if size >= 1000 {
            tags::read_cover_bytes(&path).map(|b| tags::bytes_to_data_url(&b))
        } else {
            tags::cover_thumb_data_url(&path, size)
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Some(u) = &url {
        let mut th = state.thumbs.lock().unwrap();
        if th.len() > 3000 {
            th.clear();
        }
        th.insert(key, u.clone());
    }
    Ok(url)
}

#[tauri::command]
fn export_cover(state: St, id: String, dest: String) -> Result<(), String> {
    let t = state.track(&id)?;
    let b = tags::read_cover_bytes(Path::new(&t.path)).ok_or("Brak okładki")?;
    std::fs::write(dest, b).map_err(|e| e.to_string())
}

// ───────────────────────────── Analiza (automatyczne tagowanie) ─────────────────────────────

fn job_guard(state: &AppState) -> Result<(), String> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("Trwa już inne zadanie – poczekaj lub je zatrzymaj".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    Ok(())
}

fn has_work(p: &Proposal) -> bool {
    p.changes.iter().any(|c| c.enabled) || p.cover_enabled || p.lyrics_enabled
}

async fn analyze_one(app: &AppHandle, state: &Arc<AppState>, id: &str, manual: Option<(String, Option<String>)>) -> Result<(), String> {
    let settings = state.settings();
    let Some(t) = state.set_status(id, TrackStatus::Working) else { return Ok(()) };
    emit_track(app, &t, None);
    let res = engine::analyze(&t, &settings, manual.clone()).await;
    match res.proposal {
        Some(p) => {
            let auto = manual.is_none() && settings.auto_apply && p.confidence >= settings.auto_apply_threshold;
            if auto && has_work(&p) {
                match ops::apply_proposal(&t, &p, &settings, &state.history, "auto").await {
                    Ok(nt) => {
                        state.put(nt.clone());
                        emit_track(app, &nt, None);
                    }
                    Err(e) => {
                        let mut nt = t.clone();
                        nt.status = TrackStatus::Proposed;
                        nt.error = Some(e);
                        state.put(nt.clone());
                        state.proposals.lock().unwrap().insert(id.to_string(), p.clone());
                        emit_track(app, &nt, Some(p));
                    }
                }
            } else {
                let st = if has_work(&p) || manual.is_some() { TrackStatus::Proposed } else { TrackStatus::UpToDate };
                let mut nt = t.clone();
                nt.status = st;
                nt.error = None;
                state.put(nt.clone());
                state.proposals.lock().unwrap().insert(id.to_string(), p.clone());
                emit_track(app, &nt, Some(p));
            }
        }
        None => {
            let mut nt = t.clone();
            if res.offline {
                nt.status = TrackStatus::Error;
                nt.error = Some("Brak połączenia z internetem – sprawdź sieć i spróbuj ponownie".into());
            } else {
                nt.status = TrackStatus::NotFound;
                nt.error = (!res.warnings.is_empty()).then(|| res.warnings.join("; "));
            }
            state.put(nt.clone());
            emit_track(app, &nt, None);
        }
    }
    Ok(())
}

#[tauri::command]
async fn analyze_tracks(app: AppHandle, state: St<'_>, ids: Vec<String>) -> Result<(), String> {
    let state = state.inner().clone();
    job_guard(&state)?;
    let total = ids.len();
    for id in &ids {
        if let Some(t) = state.set_status(id, TrackStatus::Queued) {
            emit_track(&app, &t, None);
        }
    }
    let concurrency = state.settings().concurrency;
    tauri::async_runtime::spawn(async move {
        let done = AtomicUsize::new(0);
        let errors = AtomicUsize::new(0);
        let progress = |d: usize, e: usize, running: bool| {
            let _ = app.emit("job-progress", JobProgress { kind: "analyze".into(), done: d, total, running, errors: e });
        };
        progress(0, 0, true);
        futures::stream::iter(ids.iter())
            .for_each_concurrent(concurrency, |id| {
                let app = app.clone();
                let state = state.clone();
                let done = &done;
                let errors = &errors;
                let progress = &progress;
                async move {
                    if state.cancel.load(Ordering::SeqCst) {
                        if let Some(t) = state.set_status(id, TrackStatus::Idle) {
                            emit_track(&app, &t, None);
                        }
                    } else if analyze_one(&app, &state, id, None).await.is_err() {
                        errors.fetch_add(1, Ordering::SeqCst);
                    }
                    let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(d, errors.load(Ordering::SeqCst), true);
                }
            })
            .await;
        state.busy.store(false, Ordering::SeqCst);
        progress(total, errors.load(Ordering::SeqCst), false);
    });
    Ok(())
}

#[tauri::command]
fn cancel_job(state: St) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn manual_search(app: AppHandle, state: St<'_>, id: String, title: String, artist: Option<String>) -> Result<Option<Proposal>, String> {
    let state = state.inner().clone();
    analyze_one(&app, &state, &id, Some((title, artist))).await?;
    let p = state.proposals.lock().unwrap().get(&id).cloned();
    Ok(p)
}

#[tauri::command]
fn get_proposal(state: St, id: String) -> Option<Proposal> {
    state.proposals.lock().unwrap().get(&id).cloned()
}

#[tauri::command]
fn update_proposal(state: St, proposal: Proposal) -> Result<(), String> {
    state.proposals.lock().unwrap().insert(proposal.track_id.clone(), proposal);
    Ok(())
}

#[tauri::command]
async fn choose_alternative(state: St<'_>, id: String, index: usize) -> Result<Proposal, String> {
    let t = state.track(&id)?;
    let mut p = state.proposals.lock().unwrap().get(&id).cloned().ok_or("Brak propozycji")?;
    engine::rebuild_with(&t, &mut p, index, &state.settings()).await;
    state.proposals.lock().unwrap().insert(id, p.clone());
    Ok(p)
}

#[tauri::command]
fn discard_proposals(app: AppHandle, state: St, ids: Vec<String>) {
    for id in ids {
        state.proposals.lock().unwrap().remove(&id);
        if let Some(t) = state.set_status(&id, TrackStatus::Idle) {
            emit_track(&app, &t, None);
        }
    }
}

/// Dodaje propozycje do kolejki zapisu. Kolejka działa w tle i zapisuje kilka plików równolegle,
/// więc kolejne akceptacje można dodawać w trakcie zapisu.
#[tauri::command]
async fn apply_proposals(app: AppHandle, state: St<'_>, ids: Vec<String>) -> Result<usize, String> {
    let state = state.inner().clone();
    let added = {
        let mut q = state.apply_queue.lock().unwrap();
        let mut n = 0;
        for id in ids {
            if !q.contains(&id) && state.proposals.lock().unwrap().contains_key(&id) {
                q.push_back(id.clone());
                n += 1;
                if let Some(t) = state.set_status(&id, TrackStatus::Queued) {
                    emit_track(&app, &t, None);
                }
            }
        }
        n
    };
    state.apply_total.fetch_add(added, Ordering::SeqCst);
    if !state.apply_running.swap(true, Ordering::SeqCst) {
        state.cancel.store(false, Ordering::SeqCst);
        tauri::async_runtime::spawn(apply_worker(app, state));
    }
    Ok(added)
}

async fn apply_worker(app: AppHandle, state: Arc<AppState>) {
    let batch = uuid::Uuid::new_v4().to_string();
    let progress = |running: bool| {
        let _ = app.emit(
            "job-progress",
            JobProgress {
                kind: "apply".into(),
                done: state.apply_done.load(Ordering::SeqCst),
                total: state.apply_total.load(Ordering::SeqCst),
                running,
                errors: state.apply_errors.load(Ordering::SeqCst),
            },
        );
    };
    progress(true);
    loop {
        let settings = state.settings();
        let parallel = settings.concurrency.clamp(2, 4);
        let chunk: Vec<String> = {
            let mut q = state.apply_queue.lock().unwrap();
            if state.cancel.load(Ordering::SeqCst) {
                for id in q.drain(..) {
                    if let Some(t) = state.set_status(&id, TrackStatus::Proposed) {
                        let p = state.proposals.lock().unwrap().get(&id).cloned();
                        emit_track(&app, &t, p);
                    }
                }
            }
            let n = parallel.min(q.len());
            q.drain(..n).collect()
        };
        if chunk.is_empty() {
            break;
        }
        futures::future::join_all(chunk.into_iter().map(|id| {
            let app = app.clone();
            let state = state.clone();
            let settings = settings.clone();
            let batch = batch.clone();
            async move {
                let p = state.proposals.lock().unwrap().get(&id).cloned();
                let (Some(p), Ok(t)) = (p, state.track(&id)) else { return };
                if let Some(w) = state.set_status(&id, TrackStatus::Working) {
                    emit_track(&app, &w, None);
                }
                match ops::apply_proposal(&t, &p, &settings, &state.history, &batch).await {
                    Ok(nt) => {
                        state.proposals.lock().unwrap().remove(&id);
                        state.put(nt.clone());
                        emit_track(&app, &nt, None);
                    }
                    Err(e) => {
                        state.apply_errors.fetch_add(1, Ordering::SeqCst);
                        let mut nt = t.clone();
                        nt.status = TrackStatus::Error;
                        nt.error = Some(e);
                        state.put(nt.clone());
                        emit_track(&app, &nt, Some(p));
                    }
                }
                state.apply_done.fetch_add(1, Ordering::SeqCst);
            }
        }))
        .await;
        progress(true);
    }
    state.apply_running.store(false, Ordering::SeqCst);
    progress(false);
    state.apply_total.store(0, Ordering::SeqCst);
    state.apply_done.store(0, Ordering::SeqCst);
    state.apply_errors.store(0, Ordering::SeqCst);
}

static COVER_SIZES: once_cell::sync::Lazy<Mutex<HashMap<String, (u32, u32, usize)>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Serialize)]
struct CoverSize {
    url: String,
    width: u32,
    height: u32,
    bytes: usize,
}

/// Sprawdza rzeczywistą rozdzielczość okładki pod adresem (pobiera obraz, wynik w pamięci podręcznej).
#[tauri::command]
async fn probe_cover(url: String) -> Result<CoverSize, String> {
    if let Some((w, h, b)) = COVER_SIZES.lock().unwrap().get(&url).cloned() {
        return Ok(CoverSize { url, width: w, height: h, bytes: b });
    }
    let mut data = sources::http::get_bytes(&url).await;
    if data.is_err() && url.contains("front-1200") {
        data = sources::http::get_bytes(&url.replace("-1200", "")).await;
    }
    let data = data?;
    let (w, h, _) = tags::image_info(&data).ok_or("To nie jest obraz")?;
    COVER_SIZES.lock().unwrap().insert(url.clone(), (w, h, data.len()));
    Ok(CoverSize { url, width: w, height: h, bytes: data.len() })
}

// ───────────────────────────── Ręczna edycja ─────────────────────────────

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditRequest {
    ids: Vec<String>,
    #[serde(default)]
    fields: HashMap<String, Option<String>>,
    cover: Option<tags::CoverOp>,
    /// Some(None) = usuń tekst, Some(Some(text)) = ustaw.
    #[serde(default, with = "double_option")]
    lyrics: Option<Option<String>>,
}

mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
        Ok(Some(Option::<String>::deserialize(d)?))
    }
}

#[tauri::command]
async fn save_edits(app: AppHandle, state: St<'_>, req: EditRequest) -> Result<Vec<Track>, String> {
    let state = state.inner().clone();
    let settings = state.settings();
    let batch = uuid::Uuid::new_v4().to_string();
    let cover = match &req.cover {
        Some(tags::CoverOp::Set { data_b64 }) => {
            use base64::Engine;
            let raw = base64::engine::general_purpose::STANDARD
                .decode(data_b64.split(',').next_back().unwrap_or(""))
                .map_err(|e| e.to_string())?;
            Some(tags::process_cover(&raw, settings.max_cover_size, settings.cover_quality)?.0)
        }
        _ => None,
    };
    let mut out = vec![];
    let mut errors = vec![];
    for id in &req.ids {
        let t = state.track(id)?;
        let path = PathBuf::from(&t.path);
        let mut w = tags::WriteRequest {
            fields: req.fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            cover: cover.clone(),
            remove_cover: matches!(req.cover, Some(tags::CoverOp::Remove)),
            lyrics: None,
        };
        let mut summary: Vec<String> = req.fields.keys().cloned().collect();
        if req.cover.is_some() && !matches!(req.cover, Some(tags::CoverOp::Keep)) {
            summary.push("cover".into());
        }
        state.history.record(&batch, &path, "edit", summary);
        if let Some(l) = &req.lyrics {
            let synced = l.as_deref().map(tags::is_synced_lyrics).unwrap_or(false);
            if settings.embed_lyrics || l.is_none() || !synced {
                w.lyrics = Some(l.clone());
            }
            let lrc = tags::lrc_path(&path);
            match l {
                Some(text) if synced && (settings.write_lrc_file || lrc.exists()) => {
                    let _ = std::fs::write(&lrc, ops::lrc_with_header(text, &t.tags, t.duration));
                }
                None => {
                    let _ = std::fs::remove_file(&lrc);
                }
                _ => {}
            }
        }
        let p2 = path.clone();
        match tauri::async_runtime::spawn_blocking(move || tags::write(&p2, &w)).await.map_err(|e| e.to_string())? {
            Ok(()) => {
                if settings.save_folder_jpg {
                    if let Some(c) = &cover {
                        ops::save_folder_cover(&path, c);
                    }
                }
                let mut nt = tags::read_track(&path);
                tags::evaluate(&mut nt, settings.min_cover_size);
                nt.status = TrackStatus::Applied;
                state.put(nt.clone());
                state.proposals.lock().unwrap().remove(id);
                emit_track(&app, &nt, None);
                out.push(nt);
            }
            Err(e) => errors.push(format!("{}: {e}", t.file_name)),
        }
    }
    if !errors.is_empty() && out.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(out)
}

#[tauri::command]
async fn fetch_cover_b64(url: String) -> Result<String, String> {
    let b = sources::http::get_bytes(&url).await?;
    tags::image_info(&b).ok_or("To nie jest obraz")?;
    Ok(tags::bytes_to_data_url(&b))
}

#[tauri::command]
fn read_image_file(path: String) -> Result<String, String> {
    if path.starts_with("content://") {
        return Err("Wybierz plik z pamięci urządzenia".into());
    }
    let b = std::fs::read(&path).map_err(|e| e.to_string())?;
    tags::image_info(&b).ok_or("To nie jest obraz")?;
    Ok(tags::bytes_to_data_url(&b))
}

#[tauri::command]
async fn search_covers(state: St<'_>, query: String) -> Result<Vec<CoverCandidate>, String> {
    let country = state.settings().itunes_country;
    let (a, b) = futures::join!(sources::itunes::search_albums(&query, &country), sources::deezer::search_albums(&query));
    let mut out = vec![];
    // Przeplatamy wyniki obu źródeł.
    let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
    for i in 0..a.len().max(b.len()) {
        if let Some(x) = b.get(i) {
            out.push(x.clone());
        }
        if let Some(x) = a.get(i) {
            out.push(x.clone());
        }
    }
    if out.is_empty() {
        return Err("Nic nie znaleziono".into());
    }
    Ok(out)
}

#[tauri::command]
async fn search_lyrics(query: String) -> Result<Vec<serde_json::Value>, String> {
    let list = sources::lrclib::search_list(&query).await.map_err(|e| e.to_string())?;
    Ok(list.into_iter().take(30).collect())
}

// ───────────────────────────── Zmiana nazw ─────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenamePreview {
    id: String,
    old: String,
    new: Option<String>,
    conflict: bool,
}

#[tauri::command]
fn rename_preview(state: St, ids: Vec<String>, pattern: String) -> Vec<RenamePreview> {
    let tr = state.tracks.lock().unwrap();
    let mut seen = std::collections::HashSet::new();
    ids.iter()
        .filter_map(|id| tr.get(id))
        .map(|t| {
            let new = ops::render_pattern(&pattern, t);
            let target = new.as_ref().map(|n| Path::new(&t.path).with_file_name(n));
            let conflict = target
                .as_ref()
                .map(|p| {
                    let key = p.to_string_lossy().to_lowercase();
                    let dup = !seen.insert(key.clone());
                    dup || (p.exists() && key != t.path.to_lowercase())
                })
                .unwrap_or(false);
            RenamePreview { id: t.id.clone(), old: t.file_name.clone(), new, conflict }
        })
        .collect()
}

#[tauri::command]
fn rename_apply(app: AppHandle, state: St, ids: Vec<String>, pattern: String) -> Result<usize, String> {
    let batch = uuid::Uuid::new_v4().to_string();
    let mut n = 0;
    let mut errors = vec![];
    for id in ids {
        let t = state.track(&id)?;
        let Some(name) = ops::render_pattern(&pattern, &t) else { continue };
        if name == t.file_name {
            continue;
        }
        let old = PathBuf::from(&t.path);
        match ops::rename_file(&old, &name) {
            Ok(new) => {
                state.history.repath(&t.path, &new.to_string_lossy());
                state.history.record_rename(&batch, &old, &new);
                let mut nt = tags::read_track(&new);
                tags::evaluate(&mut nt, state.settings().min_cover_size);
                nt.status = t.status;
                if let Some(mut p) = state.proposals.lock().unwrap().remove(&id) {
                    p.track_id = nt.id.clone();
                    state.proposals.lock().unwrap().insert(nt.id.clone(), p);
                }
                state.replace(&id, nt.clone());
                let _ = app.emit("track-replaced", Replaced { old_id: id.clone(), track: nt });
                n += 1;
            }
            Err(e) => errors.push(format!("{}: {e}", t.file_name)),
        }
    }
    if !errors.is_empty() && n == 0 {
        return Err(errors.join("\n"));
    }
    Ok(n)
}

// ───────────────────────────── Historia ─────────────────────────────

#[tauri::command]
fn get_history(state: St) -> Vec<history::Entry> {
    state.history.list()
}

#[tauri::command]
fn undo(app: AppHandle, state: St, ids: Vec<String>) -> Result<usize, String> {
    let touched = state.history.undo(&ids)?;
    let min_cover = state.settings().min_cover_size;
    for (old, new) in &touched {
        let old_id = tags::track_id(old);
        let mut nt = tags::read_track(Path::new(new));
        tags::evaluate(&mut nt, min_cover);
        let known = state.tracks.lock().unwrap().contains_key(&old_id);
        if known {
            state.replace(&old_id, nt.clone());
            let _ = app.emit("track-replaced", Replaced { old_id, track: nt });
        }
    }
    Ok(touched.len())
}

#[tauri::command]
fn clear_history(state: St) {
    state.history.clear();
}

// ───────────────────────────── Narzędzia ─────────────────────────────

#[tauri::command]
fn find_duplicates(state: St) -> Vec<Vec<String>> {
    let tr = state.tracks.lock().unwrap();
    let order = state.order.lock().unwrap();
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for id in order.iter() {
        let Some(t) = tr.get(id) else { continue };
        let (Some(a), Some(ti)) = (&t.tags.artist, &t.tags.title) else { continue };
        let key = format!("{}|{}", hints::normalize(&hints::primary_artist(a)), hints::normalize_title(ti));
        if key.len() > 2 {
            groups.entry(key).or_default().push(id.clone());
        }
    }
    let mut out: Vec<Vec<String>> = groups.into_values().filter(|g| g.len() > 1).collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

#[tauri::command]
fn write_text_file(path: String, content: String) -> Result<String, String> {
    let target = if path.is_empty() || path.starts_with("content://") {
        let dir = if cfg!(target_os = "android") {
            PathBuf::from("/storage/emulated/0/Download")
        } else {
            std::env::temp_dir()
        };
        dir.join(format!("musictagger-raport-{}.csv", chrono::Local::now().format("%Y%m%d-%H%M")))
    } else {
        PathBuf::from(path)
    };
    // BOM – Excel poprawnie wyświetli polskie znaki.
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(content.as_bytes());
    std::fs::write(&target, data).map_err(|e| e.to_string())?;
    Ok(target.to_string_lossy().into())
}

#[tauri::command]
fn delete_lrc(state: St, id: String) -> Result<Track, String> {
    let t = state.track(&id)?;
    let _ = std::fs::remove_file(tags::lrc_path(Path::new(&t.path)));
    let mut nt = tags::read_track(Path::new(&t.path));
    tags::evaluate(&mut nt, state.settings().min_cover_size);
    state.put(nt.clone());
    Ok(nt)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("musictagger"));
            let _ = std::fs::create_dir_all(&data_dir);
            let settings = Settings::load(&data_dir).sanitized();
            let state = Arc::new(AppState {
                tracks: Mutex::new(HashMap::new()),
                order: Mutex::new(vec![]),
                proposals: Mutex::new(HashMap::new()),
                settings: RwLock::new(settings),
                history: History::open(&data_dir),
                data_dir,
                cancel: AtomicBool::new(false),
                busy: AtomicBool::new(false),
                thumbs: Mutex::new(HashMap::new()),
                apply_queue: Mutex::new(Default::default()),
                apply_running: AtomicBool::new(false),
                apply_total: AtomicUsize::new(0),
                apply_done: AtomicUsize::new(0),
                apply_errors: AtomicUsize::new(0),
            });
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            app_info,
            check_update,
            download_update,
            install_update,
            get_roots,
            list_dirs,
            storage_access,
            scan_folder,
            scan_files,
            reload_track,
            get_cover,
            export_cover,
            analyze_tracks,
            cancel_job,
            manual_search,
            get_proposal,
            update_proposal,
            choose_alternative,
            discard_proposals,
            apply_proposals,
            probe_cover,
            save_edits,
            fetch_cover_b64,
            read_image_file,
            search_covers,
            search_lyrics,
            rename_preview,
            rename_apply,
            get_history,
            undo,
            clear_history,
            find_duplicates,
            write_text_file,
            delete_lrc,
        ])
        .run(tauri::generate_context!())
        .expect("błąd uruchamiania aplikacji");
}
