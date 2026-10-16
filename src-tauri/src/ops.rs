//! Operacje modyfikujące pliki: zastosowanie propozycji, okładki, teksty, zmiana nazw.

use crate::history::History;
use crate::models::*;
use crate::settings::Settings;
use crate::sources::http;
use crate::tags::{self, WriteRequest};
use once_cell::sync::Lazy;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Pobiera okładkę z listy kandydatów (z obsługą zapasowych adresów).
pub async fn download_cover(p: &Proposal) -> Option<Vec<u8>> {
    let first = p.cover_choice?;
    let order: Vec<usize> = std::iter::once(first).chain((0..p.covers.len()).filter(|i| *i != first)).collect();
    for i in order {
        let c = &p.covers[i];
        let mut urls = vec![c.url.clone()];
        if c.url.contains("coverartarchive") {
            urls.push(c.url.replace("front-1200", "front-500"));
            urls.push(c.url.replace("-1200", ""));
        } else if c.url.contains("1400x1400") {
            urls.push(c.url.replace("1400x1400", "1000x1000"));
        }
        for u in urls {
            if let Ok(b) = http::get_bytes(&u).await {
                if tags::image_info(&b).is_some() {
                    return Some(b);
                }
            }
        }
    }
    None
}

/// Dodaje nagłówek LRC (wykonawca/tytuł/album/długość) jeśli go brak.
pub fn lrc_with_header(lrc: &str, f: &TagFields, duration: f64) -> String {
    if lrc.contains("[ti:") {
        return lrc.to_string();
    }
    let mut h = String::new();
    if let Some(a) = &f.artist {
        h.push_str(&format!("[ar:{a}]\n"));
    }
    if let Some(t) = &f.title {
        h.push_str(&format!("[ti:{t}]\n"));
    }
    if let Some(al) = &f.album {
        h.push_str(&format!("[al:{al}]\n"));
    }
    if duration > 0.0 {
        let d = duration.round() as u64;
        h.push_str(&format!("[length:{:02}:{:02}]\n", d / 60, d % 60));
    }
    h.push_str("[by:MusicTagger]\n");
    format!("{h}{}", lrc.trim_start())
}

/// Zapisuje tekst: zawsze w środku pliku (chyba że wybrano wyłącznie .lrc), opcjonalnie w pliku .lrc.
pub fn write_lyrics(path: &Path, req: &mut WriteRequest, l: &LyricsResult, settings: &Settings, f: &TagFields, duration: f64) -> Result<(), String> {
    let text = if settings.prefer_synced { l.synced.clone().or(l.plain.clone()) } else { l.plain.clone().or(l.synced.clone()) };
    let lrc_only = !settings.embed_lyrics && settings.write_lrc_file && l.synced.is_some();
    if !lrc_only {
        req.lyrics = Some(text.clone());
    }
    // Tekst pochodzący z istniejącego .lrc nie jest zapisywany ponownie do .lrc.
    if settings.write_lrc_file && l.source != LRC_SOURCE {
        if let Some(sync) = &l.synced {
            tags::write_text_file(&tags::lrc_path(path), &lrc_with_header(sync, f, duration), settings.hide_lrc_files)
                .map_err(|e| format!(".lrc: {e}"))?;
        }
    }
    Ok(())
}

/// Nazwa „źródła” tekstu wczytanego z pliku .lrc obok utworu.
pub const LRC_SOURCE: &str = ".lrc";

/// Co zrobić z plikiem .lrc po przeniesieniu tekstu do pliku muzycznego.
pub fn after_lrc_merge(path: &Path, settings: &Settings) {
    if !tags::lrc_path(path).exists() {
        return;
    }
    let action = if settings.write_lrc_file && settings.lrc_after_merge == "delete" { "hide" } else { settings.lrc_after_merge.as_str() };
    let res = match action {
        "delete" => std::fs::remove_file(tags::lrc_path(path)),
        "hide" => tags::hide_lrc(path),
        _ => Ok(()),
    };
    if let Err(e) = res {
        log::warn!("{}: .lrc – {e}", path.display());
    }
}

/// Przenosi tekst z pliku .lrc do środka pliku muzycznego (z wpisem w historii).
pub fn merge_lrc(track: &Track, settings: &Settings, history: &History, batch: &str) -> Result<Track, String> {
    let path = PathBuf::from(&track.path);
    let text = tags::read_lrc(&path).ok_or("Brak pliku .lrc")?;
    let synced = tags::is_synced_lyrics(&text);
    // Zwykły tekst w tagu nie jest zastępowany niezsynchronizowanym .lrc.
    if track.lyrics_in_tag && !synced {
        return Err("Plik .lrc nie zawiera czasów".into());
    }
    history.record(batch, &path, "tag", vec![if synced { "syncedLyrics" } else { "lyrics" }.into()]);
    let req = WriteRequest { lyrics: Some(Some(text)), ..Default::default() };
    tags::write(&path, &req)?;
    after_lrc_merge(&path, settings);
    let mut t = tags::read_track(&path);
    tags::evaluate(&mut t, settings.min_cover_size);
    t.status = TrackStatus::Applied;
    Ok(t)
}

pub fn save_folder_cover(path: &Path, data: &[u8]) {
    if let Some(dir) = path.parent() {
        let exists = ["cover.jpg", "folder.jpg", "Cover.jpg", "Folder.jpg", "cover.png", "folder.png"]
            .iter()
            .any(|n| dir.join(n).exists());
        if !exists {
            let _ = std::fs::write(dir.join("cover.jpg"), data);
        }
    }
}

/// Uruchamia blokującą operację na pliku bez blokowania wątku asynchronicznego
/// (w środowisku wielowątkowym; w testach – po prostu wykonuje).
pub fn run_blocking<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(f),
        _ => f(),
    }
}

/// Gotowe (pobrane i przeskalowane) okładki – utwory z jednego albumu nie pobierają jej ponownie.
static COVER_CACHE: Lazy<Mutex<VecDeque<(String, Vec<u8>)>>> = Lazy::new(|| Mutex::new(VecDeque::new()));
const COVER_CACHE_SIZE: usize = 24;

async fn prepared_cover(p: &Proposal, settings: &Settings) -> Result<Vec<u8>, String> {
    let url = p.cover_choice.and_then(|i| p.covers.get(i)).map(|c| c.url.clone()).unwrap_or_default();
    let key = format!("{url}|{}|{}", settings.max_cover_size, settings.cover_quality);
    if let Some((_, b)) = COVER_CACHE.lock().unwrap().iter().find(|(k, _)| *k == key) {
        return Ok(b.clone());
    }
    let raw = download_cover(p).await.ok_or("Nie udało się pobrać okładki")?;
    let (max, q) = (settings.max_cover_size, settings.cover_quality);
    let jpg = tokio::task::spawn_blocking(move || tags::process_cover(&raw, max, q).map(|x| x.0))
        .await
        .map_err(|e| e.to_string())??;
    let mut c = COVER_CACHE.lock().unwrap();
    c.push_back((key, jpg.clone()));
    while c.len() > COVER_CACHE_SIZE {
        c.pop_front();
    }
    Ok(jpg)
}

/// Stosuje propozycję do pliku. Zwraca zaktualizowany utwór.
pub async fn apply_proposal(track: &Track, p: &Proposal, settings: &Settings, history: &History, batch: &str) -> Result<Track, String> {
    let path = PathBuf::from(&track.path);
    let mut req = WriteRequest::default();
    let mut summary = vec![];
    let mut merged = track.tags.clone();
    for ch in p.changes.iter().filter(|c| c.enabled) {
        req.fields.push((ch.field.clone(), ch.new.clone()));
        merged.set(&ch.field, ch.new.clone());
        summary.push(ch.field.clone());
    }
    let mut cover_bytes = None;
    if p.cover_enabled {
        let jpg = prepared_cover(p, settings).await?;
        cover_bytes = Some(jpg.clone());
        req.cover = Some(jpg);
        summary.push("cover".into());
    }
    let lyrics = p.lyrics.as_ref().filter(|_| p.lyrics_enabled);
    if let Some(l) = lyrics {
        summary.push(if l.synced.is_some() { "syncedLyrics" } else { "lyrics" }.into());
    }
    run_blocking(|| -> Result<(), String> {
        history.record(batch, &path, "tag", summary.clone());
        if let Some(l) = lyrics {
            write_lyrics(&path, &mut req, l, settings, &merged, track.duration)?;
        }
        if !req.fields.is_empty() || req.cover.is_some() || req.lyrics.is_some() {
            tags::write(&path, &req)?;
        }
        if lyrics.map(|l| l.source == LRC_SOURCE).unwrap_or(false) && req.lyrics.is_some() {
            after_lrc_merge(&path, settings);
        }
        if settings.save_folder_jpg {
            if let Some(b) = &cover_bytes {
                save_folder_cover(&path, b);
            }
        }
        Ok(())
    })?;
    let mut t = run_blocking(|| tags::read_track(&path));
    tags::evaluate(&mut t, settings.min_cover_size);
    t.status = TrackStatus::Applied;
    Ok(t)
}

const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

fn sanitize(s: &str) -> String {
    let s: String = s.chars().map(|c| if FORBIDDEN.contains(&c) || c.is_control() { '_' } else { c }).collect();
    s.trim().trim_end_matches('.').trim().to_string()
}

/// Tworzy nazwę pliku wg wzorca: %artist%, %title%, %album%, %track%, %year%, %disc%, %albumartist%.
pub fn render_pattern(pattern: &str, t: &Track) -> Option<String> {
    let f = &t.tags;
    let title = f.title.as_deref()?;
    let mut out = pattern.to_string();
    let rep = |out: &mut String, key: &str, val: String| {
        *out = out.replace(key, &sanitize(&val));
    };
    rep(&mut out, "%artist%", f.artist.clone().unwrap_or_else(|| "Unknown".into()));
    rep(&mut out, "%albumartist%", f.album_artist.clone().or(f.artist.clone()).unwrap_or_default());
    rep(&mut out, "%title%", title.to_string());
    rep(&mut out, "%album%", f.album.clone().unwrap_or_default());
    rep(&mut out, "%year%", f.year.as_deref().and_then(|y| y.get(0..4)).unwrap_or("").to_string());
    rep(&mut out, "%track%", f.track.map(|n| format!("{n:02}")).unwrap_or_default());
    rep(&mut out, "%disc%", f.disc.map(|n| n.to_string()).unwrap_or_default());
    rep(&mut out, "%genre%", f.genre.clone().unwrap_or_default());
    // Sprzątanie po pustych polach: „ - ”, „()”, podwójne spacje.
    let mut out = out.replace("()", "").replace("[]", "");
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    let out = out.trim().trim_matches(|c: char| c == '-' || c == '.' || c == '_' || c.is_whitespace()).replace(" -  - ", " - ");
    let out = out.trim_start_matches("- ").to_string();
    if out.is_empty() {
        return None;
    }
    let ext = Path::new(&t.path).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let name: String = out.chars().take(180).collect();
    Some(format!("{}.{}", name.trim(), ext))
}

pub fn rename_file(old: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new = old.with_file_name(new_name);
    if new == old {
        return Ok(new);
    }
    let same_ignoring_case = new.to_string_lossy().to_lowercase() == old.to_string_lossy().to_lowercase();
    if new.exists() && !same_ignoring_case {
        return Err(format!("Plik już istnieje: {new_name}"));
    }
    std::fs::rename(old, &new).map_err(|e| e.to_string())?;
    let lrc_old = tags::lrc_path(old);
    if lrc_old.exists() {
        let _ = std::fs::rename(&lrc_old, tags::lrc_path(&new));
    }
    let bak = tags::lrc_backup_path(old);
    if bak.exists() {
        let _ = std::fs::rename(&bak, tags::lrc_backup_path(&new));
    }
    Ok(new)
}
