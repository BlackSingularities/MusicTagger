//! Łączenie wielu źródeł tekstów: najpierw teksty z czasami (LRCLIB, NetEase),
//! potem zwykłe (Tekstowo.pl, Genius, lyrics.ovh). Każdy wynik jest sprawdzany
//! (tytuł, wykonawca, długość), a teksty z czasami muszą pasować do długości utworu.

use super::http::HttpError;
use super::{genius, lrclib, lyricsovh, netease, tekstowo};
use crate::hints;
use crate::models::LyricsResult;
use crate::settings::Settings;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Pozycja na liście wyników ręcznego wyszukiwania tekstu.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsHit {
    pub source: String,
    /// Identyfikator/adres w źródle – do pobrania tekstu na żądanie.
    #[serde(rename = "ref")]
    pub reference: String,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: Option<String>,
    pub duration: Option<f64>,
    pub synced_lyrics: Option<String>,
    pub plain_lyrics: Option<String>,
    pub instrumental: bool,
    /// Czy tekst jest już pobrany (inaczej trzeba wywołać `fetch_hit`).
    pub loaded: bool,
}

pub struct LyricsQuery<'a> {
    pub artist: &'a str,
    pub title: &'a str,
    pub album: Option<&'a str>,
    pub duration: f64,
}

/// Warianty tytułu do wyszukiwania: oryginał, bez „feat.”, bez dopisków typu „(Remastered 2011)”.
pub fn title_variants(title: &str) -> Vec<String> {
    static EXTRA: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)\s*(?:[\(\[][^\)\]]*(?:remaster|version|edit|mix|mono|stereo|wersja|radio|explicit|bonus|single)[^\)\]]*[\)\]]|\s+-\s+.*(?:remaster|version|edit|mix|mono|stereo|wersja|radio).*$)").unwrap()
    });
    let mut out = vec![title.trim().to_string()];
    let no_feat = hints::split_feat(title).0;
    let clean = EXTRA.replace_all(&no_feat, "").trim().to_string();
    for v in [no_feat, clean] {
        if !v.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&v)) {
            out.push(v);
        }
    }
    out
}

/// Czy tekst z czasami pasuje długością do utworu (inna wersja = rozjechane czasy).
pub fn synced_fits(lrc: &str, duration: f64) -> bool {
    let lines = crate::tags::parse_lrc(lrc);
    if lines.len() < 3 {
        return false;
    }
    if duration <= 0.0 {
        return true;
    }
    let last = lines.iter().map(|x| x.0).max().unwrap_or(0) as f64 / 1000.0;
    last <= duration + 12.0
}

fn hit_matches(h: &LyricsHit, q: &LyricsQuery) -> f64 {
    let ts = hints::title_similarity(&h.track_name, q.title);
    let as_ = if h.artist_name.is_empty() { 0.75 } else { hints::artist_similarity(&h.artist_name, q.artist) };
    if ts < 0.8 || as_ < 0.6 {
        return 0.0;
    }
    let ds = match h.duration {
        Some(d) if q.duration > 0.0 => hints::duration_score(d, q.duration),
        _ => 0.6,
    };
    if q.duration > 0.0 && h.duration.is_some() && ds < 0.5 {
        return 0.0;
    }
    ts * 0.45 + as_ * 0.35 + ds * 0.2
}

fn note(errs: &mut Vec<String>, e: HttpError) {
    if !matches!(e, HttpError::NotFound | HttpError::RateLimited) {
        errs.push(e.to_string());
    }
}

async fn from_lrclib(q: &LyricsQuery<'_>, errs: &mut Vec<String>) -> Option<LyricsResult> {
    let mut plain_only: Option<LyricsResult> = None;
    for t in title_variants(q.title).iter().take(2) {
        match lrclib::find(q.artist, t, q.album, q.duration).await {
            Ok(Some(mut r)) => {
                if r.instrumental {
                    return Some(r);
                }
                if let Some(s) = &r.synced {
                    if !synced_fits(s, q.duration) {
                        r.synced = None;
                    }
                }
                if r.synced.is_some() {
                    return Some(r);
                }
                if r.plain.is_some() && plain_only.is_none() {
                    plain_only = Some(r);
                }
            }
            Ok(None) => {}
            Err(e) => {
                note(errs, e);
                break;
            }
        }
    }
    plain_only
}

async fn from_netease(q: &LyricsQuery<'_>, errs: &mut Vec<String>) -> Option<LyricsResult> {
    let title = title_variants(q.title).pop().unwrap_or_default();
    let query = format!("{} {}", hints::primary_artist(q.artist), title);
    let hits = match netease::search(&query, 10).await {
        Ok(h) => h,
        Err(e) => {
            note(errs, e);
            return None;
        }
    };
    let mq = LyricsQuery { title: &title, ..*q };
    let mut scored: Vec<(f64, LyricsHit)> =
        hits.into_iter().map(|h| (hit_matches(&h, &mq), h)).filter(|(s, _)| *s > 0.0).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, h) in scored.into_iter().take(2) {
        match netease::lyrics(&h.reference).await {
            Ok((_, _, true)) => {
                return Some(LyricsResult { source: "NetEase".into(), instrumental: true, ..Default::default() });
            }
            Ok((Some(s), _, _)) if synced_fits(&s, q.duration) => {
                return Some(LyricsResult { synced: Some(s), source: "NetEase".into(), ..Default::default() });
            }
            Ok(_) => {}
            Err(e) => {
                note(errs, e);
                break;
            }
        }
    }
    None
}

async fn from_tekstowo(q: &LyricsQuery<'_>, errs: &mut Vec<String>) -> Option<String> {
    let artist = hints::primary_artist(q.artist);
    let title = title_variants(q.title).pop().unwrap_or_default();
    // Najpierw bezpośredni adres (jedno zapytanie), potem wyszukiwarka.
    match tekstowo::lyrics(&tekstowo::song_url(&artist, &title)).await {
        Ok(Some(t)) => return Some(t),
        Ok(None) | Err(HttpError::NotFound) => {}
        Err(e) => {
            note(errs, e);
            return None;
        }
    }
    let hits = match tekstowo::search(&artist, &title).await {
        Ok(h) => h,
        Err(e) => {
            note(errs, e);
            return None;
        }
    };
    let mq = LyricsQuery { artist: &artist, title: &title, ..*q };
    let best = hits.into_iter().map(|h| (hit_matches(&h, &mq), h)).filter(|(s, _)| *s > 0.0).max_by(|a, b| {
        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
    })?;
    tekstowo::lyrics(&best.1.reference).await.unwrap_or_else(|e| {
        note(errs, e);
        None
    })
}

async fn from_genius(q: &LyricsQuery<'_>, errs: &mut Vec<String>) -> Option<String> {
    let title = title_variants(q.title).pop().unwrap_or_default();
    let hits = match genius::search(&format!("{} {}", hints::primary_artist(q.artist), title)).await {
        Ok(h) => h,
        Err(e) => {
            note(errs, e);
            return None;
        }
    };
    let mq = LyricsQuery { title: &title, ..*q };
    let best = hits.into_iter().map(|h| (hit_matches(&h, &mq), h)).filter(|(s, _)| *s > 0.0).max_by(|a, b| {
        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
    })?;
    genius::lyrics(&best.1.reference).await.unwrap_or_else(|e| {
        note(errs, e);
        None
    })
}

async fn from_ovh(q: &LyricsQuery<'_>, errs: &mut Vec<String>) -> Option<String> {
    let title = title_variants(q.title).pop().unwrap_or_default();
    match lyricsovh::lyrics(&hints::primary_artist(q.artist), &title).await {
        Ok(t) => t,
        Err(e) => {
            note(errs, e);
            None
        }
    }
}

/// Szuka tekstu we wszystkich włączonych źródłach.
/// `synced_only` – utwór ma już zwykły tekst, szukamy tylko wersji z czasami.
pub async fn find(q: &LyricsQuery<'_>, settings: &Settings, synced_only: bool) -> (Option<LyricsResult>, Vec<String>) {
    let mut errs = vec![];
    // 1) Teksty z czasami – oba źródła równolegle.
    let (mut e1, mut e2) = (vec![], vec![]);
    let lr = async {
        if settings.use_lrclib {
            from_lrclib(q, &mut e1).await
        } else {
            None
        }
    };
    let ne = async {
        if settings.use_netease {
            from_netease(q, &mut e2).await
        } else {
            None
        }
    };
    let (lr, ne) = futures::join!(lr, ne);
    errs.extend(e1);
    errs.extend(e2);
    if let Some(r) = lr.as_ref().filter(|r| r.synced.is_some() || r.instrumental) {
        return (Some(r.clone()), vec![]);
    }
    if let Some(mut r) = ne {
        if !r.instrumental {
            r.plain = lr.as_ref().and_then(|l| l.plain.clone());
        }
        return (Some(r), vec![]);
    }
    if let Some(r) = lr {
        return (Some(r), vec![]);
    }
    if synced_only {
        return (None, errs);
    }

    // 2) Zwykłe teksty – równolegle, wybór wg priorytetu.
    let (mut e3, mut e4, mut e5) = (vec![], vec![], vec![]);
    let tk = async {
        if settings.use_tekstowo {
            from_tekstowo(q, &mut e3).await
        } else {
            None
        }
    };
    let gn = async {
        if settings.use_genius {
            from_genius(q, &mut e4).await
        } else {
            None
        }
    };
    let ov = async {
        if settings.use_lyricsovh {
            from_ovh(q, &mut e5).await
        } else {
            None
        }
    };
    let (tk, gn, ov) = futures::join!(tk, gn, ov);
    errs.extend(e3);
    errs.extend(e4);
    errs.extend(e5);
    // Polskie utwory – najpierw Tekstowo, pozostałe – najpierw Genius.
    let polish = hints::diacritic_count(q.artist) + hints::diacritic_count(q.title) > 0;
    let order: Vec<(&str, Option<String>)> = if polish {
        vec![("Tekstowo.pl", tk), ("Genius", gn), ("lyrics.ovh", ov)]
    } else {
        vec![("Genius", gn), ("Tekstowo.pl", tk), ("lyrics.ovh", ov)]
    };
    for (src, text) in order {
        if let Some(t) = text {
            return (Some(LyricsResult { plain: Some(t), source: src.into(), ..Default::default() }), vec![]);
        }
    }
    (None, errs)
}

/// Ręczne wyszukiwanie – lista wyników ze wszystkich źródeł.
pub async fn search_all(query: &str, settings: &Settings) -> Result<Vec<LyricsHit>, String> {
    let (artist, title) = query.split_once(" - ").map(|(a, t)| (a.trim(), t.trim())).unwrap_or(("", query.trim()));
    let lr = async {
        if !settings.use_lrclib {
            return Ok(vec![]);
        }
        lrclib::search_list(query).await.map(|l| {
            l.into_iter()
                .take(25)
                .map(|v| {
                    let st = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from).filter(|x| !x.trim().is_empty());
                    LyricsHit {
                        source: "LRCLIB".into(),
                        reference: v.get("id").map(|x| x.to_string()).unwrap_or_default(),
                        track_name: st("trackName").unwrap_or_default(),
                        artist_name: st("artistName").unwrap_or_default(),
                        album_name: st("albumName"),
                        duration: v.get("duration").and_then(|x| x.as_f64()),
                        synced_lyrics: st("syncedLyrics"),
                        plain_lyrics: st("plainLyrics"),
                        instrumental: v.get("instrumental").and_then(|x| x.as_bool()).unwrap_or(false),
                        loaded: true,
                    }
                })
                .collect::<Vec<_>>()
        })
    };
    let ne = async { if settings.use_netease { netease::search(query, 10).await } else { Ok(vec![]) } };
    let gn = async { if settings.use_genius { genius::search(query).await } else { Ok(vec![]) } };
    let tk = async {
        if settings.use_tekstowo {
            tekstowo::search(artist, if artist.is_empty() { query } else { title }).await
        } else {
            Ok(vec![])
        }
    };
    let (lr, ne, gn, tk) = futures::join!(lr, ne, gn, tk);
    let mut errs = vec![];
    let mut lists = vec![];
    for r in [lr, ne, gn, tk] {
        match r {
            Ok(l) => lists.push(l),
            Err(e) => errs.push(e.to_string()),
        }
    }
    let mut out = vec![];
    // LRCLIB (gotowe teksty) najpierw, potem przeplatane pozostałe źródła.
    if let Some(first) = lists.first().filter(|l| l.first().map(|h| h.source == "LRCLIB").unwrap_or(false)) {
        out.extend(first.iter().take(15).cloned());
        lists.remove(0);
    }
    let longest = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    for i in 0..longest {
        for l in &lists {
            if let Some(h) = l.get(i) {
                out.push(h.clone());
            }
        }
    }
    if out.is_empty() && !errs.is_empty() {
        return Err(errs.join("; "));
    }
    out.truncate(40);
    Ok(out)
}

/// Pobiera tekst wybranego wyniku (źródła, które nie zwracają tekstu w wyszukiwarce).
pub async fn fetch_hit(source: &str, reference: &str) -> Result<LyricsHit, String> {
    let mut h = LyricsHit { source: source.into(), reference: reference.into(), loaded: true, ..Default::default() };
    match source {
        "NetEase" => {
            let (s, p, i) = netease::lyrics(reference).await.map_err(|e| e.to_string())?;
            h.synced_lyrics = s;
            h.plain_lyrics = p;
            h.instrumental = i;
        }
        "Genius" => h.plain_lyrics = genius::lyrics(reference).await.map_err(|e| e.to_string())?,
        "Tekstowo" => h.plain_lyrics = tekstowo::lyrics(reference).await.map_err(|e| e.to_string())?,
        _ => return Err("Nieznane źródło".into()),
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_and_fit() {
        let v = title_variants("Hey Jude (Remastered 2015) feat. X");
        assert!(v.contains(&"Hey Jude".to_string()), "{v:?}");
        let v = title_variants("Song - Radio Edit");
        assert_eq!(v.last().unwrap(), "Song");
        let lrc = "[00:01.00] a\n[00:10.00] b\n[03:30.00] c";
        assert!(synced_fits(lrc, 215.0));
        assert!(!synced_fits(lrc, 150.0));
    }
}
