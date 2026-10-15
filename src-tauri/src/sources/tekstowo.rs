//! Tekstowo.pl – największa polska baza tekstów (bez czasów).

use super::html;
use super::http::{get_html, HttpError};
use super::lyrics::LyricsHit;
use crate::hints;
use once_cell::sync::Lazy;
use regex::Regex;

const BASE: &str = "https://www.tekstowo.pl";

/// Fragment adresu w stylu tekstowo: małe litery bez ogonków, reszta znaków → „_”.
pub fn slug(s: &str) -> String {
    let folded = hints::fold(s);
    let mut out = String::new();
    for c in folded.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

pub fn song_url(artist: &str, title: &str) -> String {
    format!("{BASE}/piosenka,{},{}.html", slug(artist), slug(title))
}

pub async fn search(artist: &str, title: &str) -> Result<Vec<LyricsHit>, HttpError> {
    static LINK: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?s)<a[^>]+href="(/piosenka,[^"]+\.html)"[^>]*>(.*?)</a>"#).unwrap());
    let plus = |s: &str| urlencoding::encode(s).replace("%20", "+");
    let url = format!("{BASE}/szukaj,wykonawca,{},tytul,{}.html", plus(artist), plus(title));
    let page = get_html("tekstowo", &url).await?;
    let mut out: Vec<LyricsHit> = vec![];
    for c in LINK.captures_iter(&page) {
        let href = format!("{BASE}{}", &c[1]);
        if out.iter().any(|h| h.reference == href) {
            continue;
        }
        let label = html::to_text(&c[2]).replace('\n', " ");
        let (a, t) = label.split_once(" - ").unwrap_or(("", label.as_str()));
        if t.trim().is_empty() {
            continue;
        }
        out.push(LyricsHit {
            source: "Tekstowo".into(),
            reference: href,
            track_name: t.trim().to_string(),
            artist_name: a.trim().to_string(),
            ..Default::default()
        });
        if out.len() >= 10 {
            break;
        }
    }
    Ok(out)
}

/// Pobiera tekst ze strony utworu.
pub async fn lyrics(url: &str) -> Result<Option<String>, HttpError> {
    if !url.starts_with(BASE) {
        return Ok(None);
    }
    let page = get_html("tekstowo", url).await?;
    Ok(extract(&page))
}

pub fn extract(page: &str) -> Option<String> {
    // Oryginał jest w #songText, tłumaczenie w #translation – bierzemy tylko oryginał.
    let from = page.find("id=\"songText\"").unwrap_or(0);
    let parts = html::divs_with(&page[from..], "class=\"inner-text\"");
    let text = html::to_text(parts.first()?);
    let low = text.to_lowercase();
    if text.trim().is_empty() || low.starts_with("tekst piosenki nie został jeszcze") || low.contains("brak tekstu piosenki") {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_and_extract() {
        assert_eq!(slug("Dawid Podsiadło"), "dawid_podsiadlo");
        assert_eq!(slug("Małomiasteczkowy"), "malomiasteczkowy");
        let page = r#"<div class="song-text" id="songText"><div class="inner-text">Raz<br />dwa &quot;trzy&quot;<br /></div></div><div id="translation"><div class="inner-text">One</div></div>"#;
        assert_eq!(extract(page).unwrap(), "Raz\ndwa \"trzy\"");
    }
}
