//! Odczyt i zapis metadanych (lofty) oraz obróbka okładek.

use crate::models::{CoverInfo, TagFields, Track, TrackStatus};
use base64::Engine;
use lofty::config::WriteOptions;
use lofty::file::FileType;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{ItemValue, Tag, TagItem, TagType};
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

/// Parsuje tekst LRC do listy (czas w ms, linia).
pub fn parse_lrc(text: &str) -> Vec<(u32, String)> {
    static TS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]").unwrap());
    let mut out = vec![];
    for line in text.lines() {
        let words = TS.replace_all(line, "").trim().to_string();
        for c in TS.captures_iter(line) {
            let m: u32 = c[1].parse().unwrap_or(0);
            let sec: u32 = c[2].parse().unwrap_or(0);
            let frac = c.get(3).map(|f| {
                let v: u32 = f.as_str().parse().unwrap_or(0);
                match f.as_str().len() {
                    1 => v * 100,
                    2 => v * 10,
                    _ => v,
                }
            });
            out.push(((m * 60 + sec) * 1000 + frac.unwrap_or(0), words.clone()));
        }
    }
    out.sort_by_key(|x| x.0);
    out
}

pub fn is_synced_lyrics(text: &str) -> bool {
    SYNCED_RE.find_iter(text).take(3).count() >= 3
}

pub fn lrc_path(path: &Path) -> PathBuf {
    path.with_extension("lrc")
}

/// Ukryta kopia .lrc na systemach bez atrybutu „ukryty” (Android/Linux): `.nazwa.lrc`.
pub fn lrc_backup_path(path: &Path) -> PathBuf {
    let lrc = lrc_path(path);
    let name = lrc.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    lrc.with_file_name(format!(".{name}"))
}

/// Zapisuje plik tekstowy, opcjonalnie jako ukryty (Windows). Zachowuje istniejący atrybut ukrycia –
/// bez tego Windows odmawia nadpisania ukrytego pliku.
pub fn write_text_file(path: &Path, text: &str, hide: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::io::Write;
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        let existing = std::fs::metadata(path).map(|m| m.file_attributes() & (HIDDEN | SYSTEM)).unwrap_or(0);
        let attrs = existing | if hide { HIDDEN } else { 0 };
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).attributes(attrs).open(path)?;
        f.write_all(text.as_bytes())
    }
    #[cfg(not(windows))]
    {
        let _ = hide;
        std::fs::write(path, text)
    }
}

/// Ukrywa istniejący plik .lrc. Windows: atrybut „ukryty”; inne systemy: zmiana nazwy na `.nazwa.lrc`
/// (odtwarzacze go wtedy nie widzą, ale kopia zostaje).
pub fn hide_lrc(audio: &Path) -> std::io::Result<()> {
    let lrc = lrc_path(audio);
    #[cfg(windows)]
    {
        let text = std::fs::read(&lrc)?;
        let text = String::from_utf8_lossy(&text).to_string();
        write_text_file(&lrc, &text, true)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&lrc, lrc_backup_path(audio))
    }
}

/// Czyta plik .lrc (UTF-8, z BOM lub bez; inne kodowania – stratnie).
pub fn read_lrc(audio: &Path) -> Option<String> {
    let b = std::fs::read(lrc_path(audio)).ok()?;
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b);
    let s = match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        // Starsze pliki .lrc bywają w Windows-1250 – zamiana bajtów > 0x7F na znaki z tej strony kodowej.
        Err(_) => b.iter().map(|&c| cp1250(c)).collect(),
    };
    let s = s.replace("\r\n", "\n");
    (!s.trim().is_empty()).then_some(s)
}

fn cp1250(c: u8) -> char {
    const HI: &str = "€\u{81}‚\u{83}„…†‡\u{88}‰Š‹ŚŤŽŹ\u{90}‘’“”•–—\u{98}™š›śťžź ˇ˘Ł¤Ą¦§¨©Ş«¬\u{AD}®Ż°±˛ł´µ¶·¸ąş»Ľ˝ľżŔÁÂĂÄĹĆÇČÉĘËĚÍÎĎĐŃŇÓÔŐÖ×ŘŮÚŰÜÝŢßŕáâăäĺćçčéęëěíîďđńňóôőö÷řůúűüýţ˙";
    if c < 0x80 {
        c as char
    } else {
        HI.chars().nth((c - 0x80) as usize).unwrap_or('?')
    }
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
                t.lyrics_in_tag = true;
            }
            // Plik .lrc z czasami jest lepszy niż zwykły tekst w tagu – pokazujemy go.
            if t.lrc_file && !t.lyrics_synced {
                if let Some(l) = read_lrc(path) {
                    if t.lyrics.is_none() || is_synced_lyrics(&l) {
                        t.lyrics_synced = is_synced_lyrics(&l);
                        t.lyrics = Some(l);
                        t.lyrics_in_tag = false;
                    }
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
    if t.lrc_file && !t.lyrics_in_tag && t.lyrics.is_some() {
        missing.push("lrcOnly".into());
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
            // Niektóre klucze (np. MusicBrainz w ID3v2 → ramka UFID) nie mają zwykłego
            // odwzorowania – wtedy wstawiamy je bez sprawdzania.
            if !tag.insert_text(key.clone(), v.clone()) {
                tag.remove_key(&key);
                tag.insert_unchecked(TagItem::new(key, ItemValue::Text(v)));
            }
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
    let file_type = file.file_type();
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

    let opts = WriteOptions::default();
    // Pliki tylko do odczytu – zgłaszamy czytelny błąd.
    if std::fs::metadata(path).map(|m| m.permissions().readonly()).unwrap_or(false) {
        return Err("Plik jest tylko do odczytu".into());
    }
    if tag_type == TagType::Id3v2 {
        // Natywny zapis ID3v2 – obsługuje też ramki specjalne (np. UFID MusicBrainz).
        let tag = file.primary_tag().cloned().ok_or("Brak tagu")?;
        let mut id3: lofty::id3::v2::Id3v2Tag = tag.into();
        // Tekst z czasami także w natywnej ramce ID3 SYLT (oprócz USLT z LRC).
        if let Some(lyr) = &req.lyrics {
            use lofty::id3::v2::{BinaryFrame, Frame, FrameId, SyncTextContentType, SynchronizedTextFrame, TimestampFormat};
            let sylt_id = FrameId::Valid("SYLT".into());
            let _ = id3.remove(&sylt_id).count();
            if let Some(text) = lyr.as_deref().filter(|t| is_synced_lyrics(t)) {
                let lines: Vec<(u32, String)> = parse_lrc(text).into_iter().filter(|(_, l)| !l.is_empty()).collect();
                if !lines.is_empty() {
                    let f = SynchronizedTextFrame::new(
                        lofty::TextEncoding::UTF8,
                        *b"und",
                        TimestampFormat::MS,
                        SyncTextContentType::Lyrics,
                        None,
                        lines,
                    );
                    if let Ok(bytes) = f.as_bytes() {
                        id3.insert(Frame::Binary(BinaryFrame::new(sylt_id, bytes)));
                    }
                }
            }
        }
        id3.save_to_path(path, opts).map_err(|e| format!("Zapis nie powiódł się: {e}"))?;
    } else {
        file.save_to_path(path, opts).map_err(|e| format!("Zapis nie powiódł się: {e}"))?;
    }
    if file_type == FileType::Wav {
        sync_riff_info(path, opts)?;
    }
    Ok(())
}

/// WAV: dane trzymamy w ID3v2 (pełne tagi, okładka, tekst), ale Eksplorator Windows i część
/// odtwarzaczy czyta tylko blok RIFF INFO – uzupełniamy go tymi samymi podstawowymi polami,
/// żeby stare wartości nie „wracały”.
fn sync_riff_info(path: &Path, opts: WriteOptions) -> Result<(), String> {
    const RIFF_FIELDS: &[&str] = &["title", "artist", "album", "year", "track", "genre", "composer", "comment"];
    let file = lofty::read_from_path(path).map_err(|e| format!("WAV: {e}"))?;
    // Brak ID3 po zapisie = wszystkie pola wyczyszczone (np. cofnięcie zmian).
    let f = file.tag(TagType::Id3v2).map(read_fields).unwrap_or_default();
    let mut info = file.tag(TagType::RiffInfo).cloned().unwrap_or_else(|| Tag::new(TagType::RiffInfo));
    for name in RIFF_FIELDS {
        apply_field(&mut info, name, f.get(name));
    }
    info.save_to_path(path, opts).map_err(|e| format!("WAV (RIFF INFO): {e}"))
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
    // Pola także z pozostałych tagów (np. RIFF INFO w WAV, ID3v1) – inaczej cofnięcie by je wymazało.
    let mut fields = tag.map(read_fields).unwrap_or_default();
    for other in file.tags() {
        if Some(other.tag_type()) != tag.map(|t| t.tag_type()) {
            merge_missing(&mut fields, &read_fields(other));
        }
    }
    Some(Snapshot {
        fields,
        cover_b64: tag
            .and_then(best_picture)
            .map(|p| base64::engine::general_purpose::STANDARD.encode(p.data())),
        lyrics: tag.and_then(|t| clean(t.get_string(&ItemKey::Lyrics))),
        lrc: read_lrc(path),
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
        Some(text) => {
            write_text_file(&lrc, text, false).map_err(|e| e.to_string())?;
            // Kopia ukryta przy przenoszeniu do pliku (Android/Linux) jest już zbędna.
            let _ = std::fs::remove_file(lrc_backup_path(path));
        }
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

#[cfg(test)]
mod lrc_tests {
    #[test]
    fn cp1250_table() {
        assert_eq!(super::cp1250(0xB3), 'ł');
        assert_eq!(super::cp1250(0xA5), 'Ą');
        assert_eq!(super::cp1250(0xEA), 'ę');
        assert_eq!(super::cp1250(0x9C), 'ś');
        assert_eq!(super::cp1250(0xBF), 'ż');
        assert_eq!(super::cp1250(0xFF), '˙');
    }
}
