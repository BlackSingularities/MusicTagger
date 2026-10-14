//! LRCLIB – darmowa baza tekstów, także zsynchronizowanych (LRC z czasami).

use super::http::{get_json, HttpError};
use crate::hints;
use crate::models::LyricsResult;
use serde_json::Value;

const API: &str = "https://lrclib.net/api";

fn to_result(v: &Value) -> LyricsResult {
    let st = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string()).filter(|x| !x.trim().is_empty());
    LyricsResult {
        plain: st("plainLyrics"),
        synced: st("syncedLyrics"),
        source: "LRCLIB".into(),
        instrumental: v.get("instrumental").and_then(|x| x.as_bool()).unwrap_or(false),
    }
}

/// Szuka tekstu – najpierw dokładne dopasowanie (z czasem trwania), potem wyszukiwarka.
pub async fn find(artist: &str, title: &str, album: Option<&str>, duration: f64) -> Result<Option<LyricsResult>, HttpError> {
    let mut url = format!(
        "{API}/get?artist_name={}&track_name={}",
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );
    if let Some(a) = album.filter(|a| !a.is_empty()) {
        url.push_str(&format!("&album_name={}", urlencoding::encode(a)));
    }
    if duration > 0.0 {
        url.push_str(&format!("&duration={}", duration.round() as u64));
    }
    match get_json::<Value>("lrclib", &url).await {
        Ok(v) => {
            let r = to_result(&v);
            if r.synced.is_some() || r.plain.is_some() || r.instrumental {
                return Ok(Some(r));
            }
        }
        Err(HttpError::NotFound) => {}
        Err(e) => return Err(e),
    }

    let url = format!(
        "{API}/search?track_name={}&artist_name={}",
        urlencoding::encode(title),
        urlencoding::encode(&hints::primary_artist(artist))
    );
    let list: Vec<Value> = match get_json("lrclib", &url).await {
        Ok(l) => l,
        Err(HttpError::NotFound) => vec![],
        Err(e) => return Err(e),
    };
    let mut best: Option<(f64, LyricsResult)> = None;
    for item in &list {
        let t = item.get("trackName").and_then(|x| x.as_str()).unwrap_or("");
        let a = item.get("artistName").and_then(|x| x.as_str()).unwrap_or("");
        let d = item.get("duration").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let ts = hints::title_similarity(t, title);
        let as_ = hints::artist_similarity(a, artist);
        if ts < 0.8 || as_ < 0.7 {
            continue;
        }
        let ds = if duration > 0.0 { hints::duration_score(d, duration) } else { 0.6 };
        if duration > 0.0 && ds < 0.5 {
            continue; // inna wersja – teksty z czasami by się rozjechały
        }
        let r = to_result(item);
        let score = ts * 0.4 + as_ * 0.3 + ds * 0.3 + if r.synced.is_some() { 0.15 } else { 0.0 };
        if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) && (r.plain.is_some() || r.synced.is_some()) {
            best = Some((score, r));
        }
    }
    Ok(best.map(|(_, r)| r))
}

/// Wyszukiwanie ręczne – lista wyników dla okna „Znajdź tekst”.
pub async fn search_list(query: &str) -> Result<Vec<Value>, HttpError> {
    let url = format!("{API}/search?q={}", urlencoding::encode(query));
    get_json("lrclib", &url).await
}
