//! NetEase Cloud Music – ogromny zbiór tekstów z czasami (LRC), także spoza Azji.

use super::http::{get_json_h, HttpError};
use super::lyrics::LyricsHit;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

const HEADERS: &[(&str, &str)] = &[("Referer", "https://music.163.com/"), ("Cookie", "appver=2.0.2; os=pc")];

pub async fn search(query: &str, limit: usize) -> Result<Vec<LyricsHit>, HttpError> {
    let url = format!(
        "https://music.163.com/api/search/get/web?s={}&type=1&offset=0&limit={limit}",
        urlencoding::encode(query)
    );
    let v: Value = get_json_h("netease", &url, HEADERS).await?;
    let songs = v.pointer("/result/songs").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    Ok(songs
        .iter()
        .filter_map(|s| {
            let id = s.get("id")?.as_u64()?;
            let artists: Vec<String> = s
                .get("artists")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|x| x.get("name")?.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(LyricsHit {
                source: "NetEase".into(),
                reference: id.to_string(),
                track_name: s.get("name")?.as_str()?.to_string(),
                artist_name: artists.join(", "),
                album_name: s.pointer("/album/name").and_then(|x| x.as_str()).map(String::from),
                duration: s.get("duration").and_then(|d| d.as_f64()).map(|d| d / 1000.0),
                ..Default::default()
            })
        })
        .collect())
}

/// Pobiera tekst utworu. Zwraca (z czasami, bez czasów, instrumentalny).
pub async fn lyrics(id: &str) -> Result<(Option<String>, Option<String>, bool), HttpError> {
    let url = format!("https://music.163.com/api/song/lyric?id={id}&lv=1&kv=1&tv=-1");
    let v: Value = get_json_h("netease", &url, HEADERS).await?;
    if v.get("nolyric").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Ok((None, None, true));
    }
    let raw = v.pointer("/lrc/lyric").and_then(|x| x.as_str()).unwrap_or("");
    if raw.contains("纯音乐，请欣赏") || raw.contains("纯音乐, 请欣赏") {
        return Ok((None, None, true));
    }
    let cleaned = clean(raw);
    if cleaned.trim().is_empty() {
        return Ok((None, None, false));
    }
    if crate::tags::is_synced_lyrics(&cleaned) {
        Ok((Some(cleaned), None, false))
    } else {
        Ok((None, Some(cleaned), false))
    }
}

/// Usuwa linie z autorami (作词/作曲/Producer…) i nagłówki dodawane przez serwis.
pub fn clean(lrc: &str) -> String {
    static CREDIT: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)^\s*(?:\[[\d:.]+\]\s*)*(作词|作詞|作曲|编曲|編曲|制作人|製作人|监制|混音|母带|和声|吉他|贝斯|鼓|录音|OP|SP|lyricist|lyrics by|composer|composed by|arranger|arranged by|producer|produced by|mixed by|mastered by|written by)\s*[:：]").unwrap()
    });
    static META: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\[(by|offset|ti|ar|al|length|re|ve):[^\]]*\]\s*$").unwrap());
    lrc.lines()
        .filter(|l| !CREDIT.is_match(l) && !META.is_match(l))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_credits() {
        let s = "[by:xyz]\n[00:00.00] 作词 : Jan\n[00:00.50] 作曲 : Ola\n[00:01.00] Pierwsza\n[00:02.00] Composer: X\n[00:03.00] Druga";
        assert_eq!(super::clean(s), "[00:01.00] Pierwsza\n[00:03.00] Druga");
    }
}
