//! Odczyt i zapis metadanych (lofty) oraz obróbka okładek.

use crate::models::{CoverInfo, TagFields, Track, TrackStatus};
use base64::Engine;
use lofty::config::WriteOptions;
use lofty::file::FileType;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{Tag, TagType};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "m4a", "mp4", "aac", "alac", "ogg", "oga", "opus", "wav", "wv", "ape", "aiff", "aif", "mpc", "spx",
];

static SYNCED_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*\[\d{1,3}:\d{2}(?:[.:]\d{1,3})?\]").unwrap());

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_synced_lyrics(text: &str) -> bool {
    SYNCED_RE.find_iter(text).take(3).count() >= 3
}

pub fn lrc_path(path: &Path) -> PathBuf {
    path.with_extension("lrc")
}

pub fn track_id(path: &str) -> String {
    // Stabilny identyfikator – hash ścieżki.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn clean(s: Option<&str>) -> Option<String> {
    s.map(|v| v.trim_matches(char::from(0)).trim().to_string()).filter(|v| !v.is_empty())
}

fn read_fields(tag: &Tag) -> TagFields {
    let s = |k: ItemKey| clean(tag.get_string(&k));
    TagFields {
        title: clean(tag.title().as_deref()),
        artist: clean(tag.artist().as_deref()),
        album: clean(tag.album().as_deref()),
        album_artist: s(ItemKey::AlbumArtist),
        year: s(ItemKey::RecordingDate)
            .or_else(|| s(ItemKey::Year))
            .or_else(|| s(ItemKey::ReleaseDate))
            .or_else(|| s(ItemKey::OriginalReleaseDate)),
        track: tag.track(),
        track_total: tag.track_total(),
        disc: tag.disk(),
        disc_total: tag.disk_total(),
        genre: clean(tag.genre().as_deref()),
        composer: s(ItemKey::Composer),
        label: s(ItemKey::Label),
        isrc: s(ItemKey::Isrc),
        comment: clean(tag.comment().as_deref()),
        bpm: s(ItemKey::Bpm).or_else(|| s(ItemKey::IntegerBpm)),
        mb_recording_id: s(ItemKey::MusicBrainzRecordingId),
        mb_release_id: s(ItemKey::MusicBrainzReleaseId),
    }
}

fn merge_missing(base: &mut TagFields, other: &TagFields) {
    for name in crate::models::FIELD_NAMES {
        if base.get(name).is_none() {
            if let Some(v) = other.get(name) {
                base.set(name, Some(v));
            }
        }
    }
}

pub fn image_info(data: &[u8]) -> Option<(u32, u32, String)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?;
    let fmt = reader.format();
    let (w, h) = reader.into_dimensions().ok()?;
    let mime = match fmt {
        Some(image::ImageFormat::Png) => "image/png",
        Some(image::ImageFormat::WebP) => "image/webp",
        Some(image::ImageFormat::Gif) => "image/gif",
        Some(image::ImageFormat::Bmp) => "image/bmp",
        _ => "image/jpeg",
    };
    Some((w, h, mime.to_string()))
}

fn best_picture(tag: &Tag) -> Option<&Picture> {
    tag.get_picture_type(PictureType::CoverFront).or_else(|| tag.pictures().first())
}

/// Pełny odczyt pliku do struktury `Track`.
pub fn read_track(path: &Path) -> Track {
    let p = path.to_string_lossy().to_string();
    let meta = std::fs::metadata(path).ok();
    let mut t = Track {
        id: track_id(&p),
        path: p,
        file_name: path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        folder: path.parent().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        format: path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default(),
        size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        modified: meta
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        ..Default::default()
    };
    t.lrc_file = lrc_path(path).exists();

    match lofty::read_from_path(path) {
        Ok(file) => {
            let props = file.properties();
            t.duration = props.duration().as_secs_f64();
            t.bitrate = props.audio_bitrate().or(props.overall_bitrate());
            t.sample_rate = props.sample_rate();
            t.channels = props.channels();
            let primary = file.primary_tag();
            let mut fields = primary.map(read_fields).unwrap_or_default();
            for tag in file.tags() {
                if Some(tag.tag_type()) != primary.map(|p| p.tag_type()) {
                    merge_missing(&mut fields, &read_fields(tag));
                }
            }
            t.tags = fields;
            let pic = primary.and_then(best_picture).or_else(|| file.tags().iter().find_map(best_picture));
            if let Some(pic) = pic {
                let (w, h, mime) = image_info(pic.data()).unwrap_or((0, 0, "image/jpeg".into()));
                t.cover = Some(CoverInfo { width: w, height: h, mime, bytes: pic.data().len() });
            }
            let lyr = primary
                .and_then(|tg| clean(tg.get_string(&ItemKey::Lyrics)))
                .or_else(|| file.tags().iter().find_map(|tg| clean(tg.get_string(&ItemKey::Lyrics))));
            if let Some(l) = lyr {
                t.lyrics_synced = is_synced_lyrics(&l);
                t.lyrics = Some(l);
            } else if t.lrc_file {
                if let Ok(l) = std::fs::read_to_string(lrc_path(path)) {
                    t.lyrics_synced = is_synced_lyrics(&l);
                    t.lyrics = Some(l);
                }
            }
        }
        Err(e) => {
            t.status = TrackStatus::Error;
            t.error = Some(e.to_string());
        }
    }
    evaluate(&mut t, 500);
    t
}

/// Wylicza listę braków i ocenę kompletności (0–100).
pub fn evaluate(t: &mut Track, min_cover: u32) {
    let mut missing = vec![];
    let mut score: u32 = 0;
    let checks: [(&str, u32); 7] =
        [("title", 20), ("artist", 20), ("album", 14), ("year", 8), ("track", 6), ("genre", 6), ("albumArtist", 4)];
    for (f, w) in checks {
        if t.tags.get(f).is_some() {
            score += w;
        } else {
            missing.push(f.to_string());
        }
    }
    match &t.cover {
        Some(c) if c.width >= min_cover || c.width == 0 => score += 13,
        Some(_) => {
            score += 7;
            missing.push("coverSmall".into());
        }
        None => missing.push("cover".into()),
    }
    match (&t.lyrics, t.lyrics_synced) {
        (Some(_), true) => score += 9,
        (Some(_), false) => {
            score += 5;
            missing.push("syncedLyrics".into());
        }
        (None, _) => missing.push("lyrics".into()),
    }
    t.missing = missing;
    t.score = score.min(100) as u8;
}

pub fn read_cover_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = lofty::read_from_path(path).ok()?;
    let pic = file.primary_tag().and_then(best_picture).or_else(|| file.tags().iter().find_map(best_picture))?;
    Some(pic.data().to_vec())
}

/// Miniatura okładki jako data-URL JPEG.
pub fn cover_thumb_data_url(path: &Path, size: u32) -> Option<String> {
    let data = read_cover_bytes(path)?;
    let img = image::load_from_memory(&data).ok()?;
    let img = img.thumbnail(size, size).to_rgb8();
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
    img.write_with_encoder(enc).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
}

pub fn bytes_to_data_url(data: &[u8]) -> String {
    let mime = image_info(data).map(|i| i.2).unwrap_or_else(|| "image/jpeg".into());
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(data))
}

/// Skaluje okładkę do `max` px i koduje jako JPEG.
pub fn process_cover(data: &[u8], max: u32, quality: u8) -> Result<(Vec<u8>, u32, u32), String> {
    let img = image::load_from_memory(data).map_err(|e| format!("Nieprawidłowy obraz: {e}"))?;
    let img = if img.width() > max || img.height() > max {
        img.resize(max, max, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    rgb.write_with_encoder(enc).map_err(|e| e.to_string())?;
    Ok((out, w, h))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum CoverOp {
    Keep,
    Set { data_b64: String },
    Remove,
}

/// Zestaw zmian do zapisania w pliku.
#[derive(Debug, Clone, Default)]
pub struct WriteRequest {
    /// Pola do zmiany (None = usuń pole).
    pub fields: Vec<(String, Option<String>)>,
    pub cover: Option<Vec<u8>>,
    pub remove_cover: bool,
    /// Some(None) = usuń tekst.
    pub lyrics: Option<Option<String>>,
}

fn set_text(tag: &mut Tag, key: ItemKey, v: Option<String>) {
    match v {
        Some(v) => {
            tag.insert_text(key, v);
        }
        None => tag.remove_key(&key),
    }
}

fn apply_field(tag: &mut Tag, name: &str, value: Option<String>) {
    let num = value.as_ref().and_then(|s| s.split('/').next()?.trim().parse::<u32>().ok());
    match name {
        "title" => match value {
            Some(v) => tag.set_title(v),
            None => tag.remove_title(),
        },
        "artist" => match value {
            Some(v) => tag.set_artist(v),
            None => tag.remove_artist(),
        },
        "album" => match value {
            Some(v) => tag.set_album(v),
            None => tag.remove_album(),
        },
        "genre" => match value {
            Some(v) => tag.set_genre(v),
            None => tag.remove_genre(),
        },
        "comment" => match value {
            Some(v) => tag.set_comment(v),
            None => tag.remove_comment(),
        },
        "track" => match num {
            Some(n) => tag.set_track(n),
            None => tag.remove_track(),
        },
        "trackTotal" => match num {
            Some(n) => tag.set_track_total(n),
            None => tag.remove_track_total(),
        },
        "disc" => match num {
            Some(n) => tag.set_disk(n),
            None => tag.remove_disk(),
        },
        "discTotal" => match num {
            Some(n) => tag.set_disk_total(n),
            None => tag.remove_disk_total(),
        },
        "year" => {
            tag.remove_key(&ItemKey::Year);
            tag.remove_key(&ItemKey::ReleaseDate);
            set_text(tag, ItemKey::RecordingDate, value);
        }
        "albumArtist" => set_text(tag, ItemKey::AlbumArtist, value),
        "composer" => set_text(tag, ItemKey::Composer, value),
        "label" => set_text(tag, ItemKey::Label, value),
        "isrc" => set_text(tag, ItemKey::Isrc, value),
        "bpm" => {
            tag.remove_key(&ItemKey::IntegerBpm);
            set_text(tag, ItemKey::Bpm, value)
        }
        "mbRecordingId" => set_text(tag, ItemKey::MusicBrainzRecordingId, value),
        "mbReleaseId" => set_text(tag, ItemKey::MusicBrainzReleaseId, value),
        _ => {}
    }
}

/// Zapisuje zmiany do pliku. Zwraca błąd czytelny dla użytkownika.
pub fn write(path: &Path, req: &WriteRequest) -> Result<(), String> {
    let mut file = lofty::read_from_path(path).map_err(|e| format!("Nie można otworzyć pliku: {e}"))?;
    let tag_type = file.primary_tag_type();
    if file.primary_tag().is_none() {
        // Przenieś dane z innych tagów (np. ID3v1), żeby nic nie zginęło.
        let mut tag = Tag::new(tag_type);
        if let Some(other) = file.first_tag() {
            let f = read_fields(other);
            for name in crate::models::FIELD_NAMES {
                if let Some(v) = f.get(name) {
                    apply_field(&mut tag, name, Some(v));
                }
            }
        }
        file.insert_tag(tag);
    }
    let tag = file.primary_tag_mut().ok_or("Brak obsługi tagów dla tego formatu")?;

    for (name, value) in &req.fields {
        apply_field(tag, name, value.clone());
    }
    if req.remove_cover {
        tag.remove_picture_type(PictureType::CoverFront);
        while tag.picture_count() > 0 {
            tag.remove_picture(0);
        }
    }
    if let Some(data) = &req.cover {
        tag.remove_picture_type(PictureType::CoverFront);
        let mime = match image_info(data).map(|i| i.2).as_deref() {
            Some("image/png") => MimeType::Png,
            _ => MimeType::Jpeg,
        };
        tag.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(mime), None, data.clone()));
    }
    if let Some(lyr) = &req.lyrics {
        set_text(tag, ItemKey::Lyrics, lyr.clone().filter(|s| !s.trim().is_empty()));
    }

    let opts = if tag_type == TagType::Id3v2 { WriteOptions::default().use_id3v23(true) } else { WriteOptions::default() };
    // Pliki tylko do odczytu – zgłaszamy czytelny błąd.
    if std::fs::metadata(path).map(|m| m.permissions().readonly()).unwrap_or(false) {
        return Err("Plik jest tylko do odczytu".into());
    }
    file.save_to_path(path, opts).map_err(|e| format!("Zapis nie powiódł się: {e}"))?;
    Ok(())
}

/// Migawka stanu pliku przed zmianą (do cofania).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub fields: TagFields,
    pub cover_b64: Option<String>,
    pub lyrics: Option<String>,
    pub lrc: Option<String>,
}

pub fn snapshot(path: &Path) -> Option<Snapshot> {
    let file = lofty::read_from_path(path).ok()?;
    let tag = file.primary_tag();
    Some(Snapshot {
        fields: tag.map(read_fields).unwrap_or_default(),
        cover_b64: tag
            .and_then(best_picture)
            .map(|p| base64::engine::general_purpose::STANDARD.encode(p.data())),
        lyrics: tag.and_then(|t| clean(t.get_string(&ItemKey::Lyrics))),
        lrc: std::fs::read_to_string(lrc_path(path)).ok(),
    })
}

pub fn restore(path: &Path, snap: &Snapshot) -> Result<(), String> {
    let req = WriteRequest {
        fields: crate::models::FIELD_NAMES.iter().map(|n| (n.to_string(), snap.fields.get(n))).collect(),
        cover: snap
            .cover_b64
            .as_ref()
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()),
        remove_cover: snap.cover_b64.is_none(),
        lyrics: Some(snap.lyrics.clone()),
    };
    write(path, &req)?;
    let lrc = lrc_path(path);
    match &snap.lrc {
        Some(text) => std::fs::write(&lrc, text).map_err(|e| e.to_string())?,
        None => {
            let _ = std::fs::remove_file(&lrc);
        }
    }
    Ok(())
}

pub fn file_type_supported(path: &Path) -> bool {
    lofty::probe::Probe::open(path)
        .ok()
        .and_then(|p| p.guess_file_type().ok())
        .and_then(|p| p.file_type())
        .map(|ft| !matches!(ft, FileType::Custom(_)))
        .unwrap_or(false)
}
