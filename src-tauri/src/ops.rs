//! Operacje modyfikujące pliki: zastosowanie propozycji, okładki, teksty, zmiana nazw.

use crate::history::History;
use crate::models::*;
use crate::settings::Settings;
use crate::sources::http;
use crate::tags::{self, WriteRequest};
use std::path::{Path, PathBuf};

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

/// Zapisuje tekst: w tagu i/lub w pliku .lrc obok utworu.
pub fn write_lyrics(path: &Path, req: &mut WriteRequest, l: &LyricsResult, settings: &Settings, f: &TagFields, duration: f64) -> Result<(), String> {
    let text = if settings.prefer_synced { l.synced.clone().or(l.plain.clone()) } else { l.plain.clone().or(l.synced.clone()) };
    if settings.embed_lyrics {
        req.lyrics = Some(text.clone());
    }
    if settings.write_lrc_file {
        if let Some(sync) = &l.synced {
            std::fs::write(tags::lrc_path(path), lrc_with_header(sync, f, duration)).map_err(|e| format!(".lrc: {e}"))?;
        }
    }
    Ok(())
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
        if let Some(raw) = download_cover(p).await {
            let (jpg, _, _) = tags::process_cover(&raw, settings.max_cover_size, settings.cover_quality)?;
            cover_bytes = Some(jpg.clone());
            req.cover = Some(jpg);
            summary.push("cover".into());
        } else {
            return Err("Nie udało się pobrać okładki".into());
        }
    }
    history.record(batch, &path, "tag", summary.clone());
    if p.lyrics_enabled {
        if let Some(l) = &p.lyrics {
            write_lyrics(&path, &mut req, l, settings, &merged, track.duration)?;
            summary.push(if l.synced.is_some() { "syncedLyrics" } else { "lyrics" }.into());
        }
    }
    if !req.fields.is_empty() || req.cover.is_some() || req.lyrics.is_some() {
        let path2 = path.clone();
        tokio::task::spawn_blocking(move || tags::write(&path2, &req)).await.map_err(|e| e.to_string())??;
    }
    if settings.save_folder_jpg {
        if let Some(b) = &cover_bytes {
            save_folder_cover(&path, b);
        }
    }
    let mut t = tags::read_track(&path);
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
    Ok(new)
}
