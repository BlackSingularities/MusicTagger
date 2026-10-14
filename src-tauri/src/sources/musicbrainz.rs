//! MusicBrainz – główne, otwarte źródło metadanych + Cover Art Archive.

use super::http::{get_json, lucene_escape, HttpError};
use crate::models::{Candidate, TagFields};
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::Mutex;

const API: &str = "https://musicbrainz.org/ws/2";

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string()).filter(|x| !x.is_empty())
}

pub fn credit_string(credits: Option<&Value>) -> Option<String> {
    let arr = credits?.as_array()?;
    let mut out = String::new();
    for c in arr {
        out.push_str(c.get("name").and_then(|n| n.as_str()).unwrap_or(""));
        out.push_str(c.get("joinphrase").and_then(|n| n.as_str()).unwrap_or(""));
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

fn year_of(date: Option<String>) -> Option<String> {
    date.and_then(|d| d.get(0..4).map(|y| y.to_string())).filter(|y| y.chars().all(|c| c.is_ascii_digit()))
}

/// Buduje kandydatów (po jednym na wydanie) z obiektu nagrania.
pub fn candidates_from_recording(rec: &Value, base_score: f64) -> Vec<Candidate> {
    let title = s(rec, "title");
    let artist = credit_string(rec.get("artist-credit"));
    let length = rec.get("length").and_then(|l| l.as_f64()).map(|ms| ms / 1000.0);
    let isrc = rec.get("isrcs").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str()).map(String::from);
    let first_year = year_of(s(rec, "first-release-date"));
    let rec_id = s(rec, "id");
    let disamb = s(rec, "disambiguation");
    let mut out = vec![];
    let releases = rec.get("releases").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    if releases.is_empty() {
        out.push(Candidate {
            source: "MusicBrainz".into(),
            fields: TagFields {
                title: title.clone(),
                artist: artist.clone(),
                year: first_year.clone(),
                isrc: isrc.clone(),
                mb_recording_id: rec_id.clone(),
                ..Default::default()
            },
            duration: length,
            disambiguation: disamb.clone(),
            score: base_score,
            ..Default::default()
        });
        return out;
    }
    for rel in releases.iter().take(25) {
        let rg = rel.get("release-group");
        let media = rel.get("media").and_then(|m| m.as_array());
        let medium = media.and_then(|m| m.first());
        let track_no = medium
            .and_then(|m| m.get("track").or_else(|| m.get("tracks")))
            .and_then(|t| t.as_array())
            .and_then(|t| t.first())
            .and_then(|t| t.get("number").and_then(|n| n.as_str()).and_then(|n| n.parse::<u32>().ok()));
        let track_total = medium.and_then(|m| m.get("track-count")).and_then(|c| c.as_u64()).map(|c| c as u32);
        let disc = medium.and_then(|m| m.get("position")).and_then(|c| c.as_u64()).map(|c| c as u32);
        let album_artist = credit_string(rel.get("artist-credit")).or_else(|| artist.clone());
        let rel_year = year_of(s(rel, "date"));
        let rg_id = rg.and_then(|g| s(g, "id"));
        let secondary: Vec<String> = rg
            .and_then(|g| g.get("secondary-types"))
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let rel_id = s(rel, "id");
        out.push(Candidate {
            source: "MusicBrainz".into(),
            fields: TagFields {
                title: title.clone(),
                artist: artist.clone(),
                album: s(rel, "title"),
                album_artist,
                year: rel_year.or_else(|| first_year.clone()),
                track: track_no,
                track_total,
                disc,
                isrc: isrc.clone(),
                mb_recording_id: rec_id.clone(),
                mb_release_id: rel_id.clone(),
                ..Default::default()
            },
            duration: length,
            cover_url: rel_id.as_ref().map(|id| format!("https://coverartarchive.org/release/{id}/front-1200")),
            cover_thumb: rel_id.as_ref().map(|id| format!("https://coverartarchive.org/release/{id}/front-250")),
            mb_release_group: rg_id,
            release_type: rg.and_then(|g| s(g, "primary-type")),
            secondary_types: secondary,
            release_status: s(rel, "status"),
            disambiguation: disamb.clone(),
            ext_id: None,
            score: base_score,
        });
        // Zachowujemy pierwszy rok wydania nagrania dla „oryginalnego” wydania.
        if let Some(last) = out.last_mut() {
            if last.fields.year.is_none() {
                last.fields.year = first_year.clone();
            }
        }
    }
    out
}

/// Wyszukiwanie nagrań po tytule i wykonawcy.
pub async fn search(title: &str, artist: Option<&str>) -> Result<Vec<(Value, f64)>, HttpError> {
    let mut q = format!("recording:\"{}\"", lucene_escape(title));
    if let Some(a) = artist.filter(|a| !a.is_empty()) {
        q.push_str(&format!(" AND artist:\"{}\"", lucene_escape(a)));
    }
    let url = format!("{API}/recording?query={}&limit=15&fmt=json", urlencoding::encode(&q));
    let v: Value = get_json("musicbrainz", &url).await?;
    Ok(v.get("recordings")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .map(|r| (r.clone(), r.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) / 100.0))
                .collect()
        })
        .unwrap_or_default())
}

/// Swobodne wyszukiwanie (gdy znamy tylko „coś” z nazwy pliku).
pub async fn search_free(text: &str) -> Result<Vec<(Value, f64)>, HttpError> {
    let url = format!("{API}/recording?query={}&limit=10&fmt=json", urlencoding::encode(&lucene_escape(text)));
    let v: Value = get_json("musicbrainz", &url).await?;
    Ok(v.get("recordings")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().map(|r| (r.clone(), r.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) / 100.0)).collect())
        .unwrap_or_default())
}

static REC_CACHE: Lazy<Mutex<HashMap<String, Value>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub async fn lookup_recording(id: &str) -> Result<Value, HttpError> {
    if let Some(v) = REC_CACHE.lock().await.get(id) {
        return Ok(v.clone());
    }
    let url = format!("{API}/recording/{id}?inc=artist-credits+releases+release-groups+media+isrcs&fmt=json");
    let v: Value = get_json("musicbrainz", &url).await?;
    REC_CACHE.lock().await.insert(id.to_string(), v.clone());
    Ok(v)
}

#[derive(Debug, Clone, Default)]
pub struct ReleaseDetails {
    pub label: Option<String>,
    pub disc_total: Option<u32>,
    pub genre: Option<String>,
    pub original_year: Option<String>,
    pub has_front: bool,
}

static REL_CACHE: Lazy<Mutex<HashMap<String, ReleaseDetails>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Szczegóły wydania: wytwórnia, liczba płyt, gatunek (z release-group), okładka.
pub async fn release_details(release_id: &str) -> Option<ReleaseDetails> {
    if let Some(d) = REL_CACHE.lock().await.get(release_id) {
        return Some(d.clone());
    }
    let url = format!("{API}/release/{release_id}?inc=labels+release-groups+genres+tags+media&fmt=json");
    let v: Value = get_json("musicbrainz", &url).await.ok()?;
    let mut d = ReleaseDetails {
        label: v
            .get("label-info")
            .and_then(|l| l.as_array())
            .and_then(|a| a.iter().find_map(|li| li.get("label").and_then(|l| s(l, "name"))))
            .filter(|l| !l.eq_ignore_ascii_case("[no label]")),
        disc_total: v.get("media").and_then(|m| m.as_array()).map(|m| m.len() as u32),
        has_front: v
            .get("cover-art-archive")
            .and_then(|c| c.get("front"))
            .and_then(|f| f.as_bool())
            .unwrap_or(false),
        ..Default::default()
    };
    let mut genre = best_genre(&v);
    if let Some(rg) = v.get("release-group") {
        d.original_year = year_of(s(rg, "first-release-date"));
        if genre.is_none() {
            if let Some(rg_id) = s(rg, "id") {
                let url = format!("{API}/release-group/{rg_id}?inc=genres+tags&fmt=json");
                if let Ok(g) = get_json::<Value>("musicbrainz", &url).await {
                    genre = best_genre(&g);
                }
            }
        }
    }
    d.genre = genre;
    REL_CACHE.lock().await.insert(release_id.to_string(), d.clone());
    Some(d)
}

fn best_genre(v: &Value) -> Option<String> {
    let pick = |key: &str| -> Option<(String, i64)> {
        v.get(key)?
            .as_array()?
            .iter()
            .filter_map(|g| Some((g.get("name")?.as_str()?.to_string(), g.get("count").and_then(|c| c.as_i64()).unwrap_or(0))))
            .max_by_key(|(_, c)| *c)
    };
    pick("genres").filter(|(_, c)| *c > 0).map(|(n, _)| title_case(&n))
}

pub fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .replace("Hip Hop", "Hip-Hop")
        .replace("R&b", "R&B")
}
