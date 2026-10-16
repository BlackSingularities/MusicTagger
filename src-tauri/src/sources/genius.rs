//! Genius – największa baza tekstów (bez czasów). Wyszukiwarka JSON + tekst ze strony utworu.

use super::html;
use super::http::{get_html, get_json_h, HttpError, BROWSER_UA};
use super::lyrics::LyricsHit;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

pub async fn search(query: &str) -> Result<Vec<LyricsHit>, HttpError> {
    let url = format!("https://genius.com/api/search/song?per_page=8&q={}", urlencoding::encode(query));
    let v: Value = get_json_h("genius", &url, &[("User-Agent", BROWSER_UA)]).await?;
    let sections = v.pointer("/response/sections").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let mut out = vec![];
    for sec in sections {
        for hit in sec.get("hits").and_then(|h| h.as_array()).cloned().unwrap_or_default() {
            let r = hit.get("result").cloned().unwrap_or(Value::Null);
            let (Some(url), Some(title)) = (r.get("url").and_then(|x| x.as_str()), r.get("title").and_then(|x| x.as_str())) else {
                continue;
            };
            if !url.ends_with("-lyrics") || out.iter().any(|h: &LyricsHit| h.reference == url) {
                continue;
            }
            let artist = r
                .get("artist_names")
                .and_then(|x| x.as_str())
                .or_else(|| r.pointer("/primary_artist/name").and_then(|x| x.as_str()))
                .unwrap_or("");
            out.push(LyricsHit {
                source: "Genius".into(),
                reference: url.to_string(),
                track_name: title.to_string(),
                artist_name: artist.to_string(),
                ..Default::default()
            });
        }
    }
    Ok(out)
}

/// Pobiera tekst ze strony utworu w Genius.
pub async fn lyrics(url: &str) -> Result<Option<String>, HttpError> {
    if !url.starts_with("https://genius.com/") {
        return Ok(None);
    }
    let page = get_html("genius", url).await?;
    Ok(extract(&page))
}

pub fn extract(page: &str) -> Option<String> {
    static SECTION: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[[^\]]{1,60}\]$").unwrap());
    let parts = html::divs_with(page, "data-lyrics-container=\"true\"");
    if parts.is_empty() {
        return None;
    }
    let mut text = String::new();
    for p in parts {
        let p = html::remove_divs_with(&p, "data-exclude-from-selection");
        text.push_str(&html::to_text(&p));
        text.push('\n');
    }
    // Nagłówki sekcji ([Refren], [Verse 1]) zastępujemy pustą linią między zwrotkami.
    let lines: Vec<&str> = text.lines().map(|l| if SECTION.is_match(l.trim()) { "" } else { l }).collect();
    let out = html::tidy_lines(&lines.join("\n"));
    let out = out.trim_start_matches('\n').to_string();
    (!out.trim().is_empty() && !out.contains("Lyrics for this song have yet to be released")).then_some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_lyrics() {
        let page = r#"<html><div data-lyrics-container="true" class="Lyrics__Container"><div data-exclude-from-selection="true">5 Contributors<div>Song Lyrics</div></div>[Zwrotka 1]<br/>Linia &#x27;jeden&#x27;<br/><a><span>Linia dwa</span></a><br/><br/>[Refren]<br/>Refren</div></html>"#;
        assert_eq!(super::extract(page).unwrap(), "Linia 'jeden'\nLinia dwa\n\nRefren");
    }
}
